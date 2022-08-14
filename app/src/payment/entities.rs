//! Handles the logic behind outgoing Lightning payments. Lightning payments require two steps:
//! - reserving user funds via [`Payment::create`], and
//! - sending the Lightning payment via [`Payment::send`].

use crate::auth;
use crate::balance;
use crate::balance::Balance;
use crate::btc;
use crate::cash_limits;
use crate::cash_limits::CashLimits;
use crate::concurrency;
use crate::ln;
use crate::user;
use chrono::DateTime;
use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0:?}")]
    LimitsViolated(#[from] cash_limits::Error),
    #[error("invalid invoice")]
    InvalidInvoice(#[from] ln::InvoiceError),
    #[error("amount has been specified both in the invoice and explicitly")]
    AmountSpecifiedTwice,
    #[error("amount has not been specified")]
    AmountNotSpecified,
    #[error("{0:?}")]
    PaymentError(#[from] ln::PaymentError),
    #[error("{0:?}")]
    ConcurrencyConflict(#[from] concurrency::ConflictError),
    #[error("{0:?}")]
    InsufficientBalance(#[from] balance::InsufficientBalance),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Id(pub Uuid);

/// Represents an outgoing Lightning payment.
/// TODO Document the methods, the order in which they are called, and why. They're pretty complex
/// here.
#[derive(Debug)]
pub struct Payment {
    pub id: Id,
    pub token_id: auth::TokenId,
    pub user_id: user::Id,
    pub amount: btc::MilliSats,
    pub invoice: ln::RawInvoice,
    pub fee: Option<btc::MilliSats>,
    pub reservation_id: Option<balance::ReservationId>,
    pub created: DateTime<Utc>,
    pub status: Status,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    New,
    Ready,
    Failed {
        reason: String,
        timestamp: DateTime<Utc>,
    },
    Succeeded {
        timestamp: DateTime<Utc>,
    },
}

impl Payment {
    /// Creates a new payment. This cannot cause a concurrency conflict.
    pub(crate) fn create(
        grant: &auth::SpendGrant,
        invoice: ln::RawInvoice,
        amount: Option<btc::MilliSats>,
        limits: &CashLimits,
        daily_total: btc::MilliSats,
    ) -> Result<Self, Error> {
        let amount = match (invoice.parse()?.amount_milli_satoshis(), amount) {
            (Some(_), Some(_)) => Err(Error::AmountSpecifiedTwice),
            (Some(amount), None) => Ok(btc::MilliSats(amount.try_into().unwrap())),
            (None, Some(amount)) => Ok(amount),
            (None, None) => Err(Error::AmountNotSpecified),
        }?;
        limits.check(cash_limits::Amounts {
            amount,
            daily_total,
        })?;
        Ok(Self {
            id: Id(Uuid::new_v4()),
            token_id: grant.token_id,
            user_id: grant.user_id,
            amount,
            invoice,
            reservation_id: None,
            fee: None,
            created: Utc::now(),
            status: Status::New,
        })
    }

    /// Determines the routing fee and reserves user funds.
    pub(crate) async fn prepare(
        &mut self,
        node: &mut ln::Node,
        balance: &mut Balance,
    ) -> Result<balance::Reservation, Error> {
        if self.status != Status::New {
            panic!("payment {:?} is not new", self.id);
        }
        if self.user_id != balance.user_id() {
            panic!(
                "user id {:?} does not match payment {:?} user id {:?}",
                balance.user_id(),
                self.id,
                self.user_id
            );
        }
        match node
            .probe_fee(&self.invoice.parse().unwrap(), Some(self.amount))
            .await
        {
            Ok(fee) => {
                let reservation = balance.reserve(self.amount + fee)?;
                self.fee = Some(fee);
                self.reservation_id = Some(reservation.id);
                self.status = Status::Ready;
                Ok(reservation)
            }
            Err(e) => {
                self.fail(&e);
                Err(Error::PaymentError(e))
            }
        }
    }

    /// Attempts to fulfill the payment. If sending succeeds, the payment is advanced into
    /// [`PaymentStatus::Succeeded`]. If sending fails, the payment is advanced into
    /// [`PaymentStatus::Failed`] and the balance reservation is refunded to the user.
    /// Both success and failure are final, and this method can't be called again afterwards.
    pub(crate) async fn send(
        &mut self,
        node: &mut ln::Node,
        balance: &mut Balance,
        reservation: &mut balance::Reservation,
    ) -> Result<(), Error> {
        if self.user_id != balance.user_id() {
            panic!(
                "balance user id {:?} does not match user id {:?} for payment {:?}",
                balance.user_id(),
                self.user_id,
                self.id
            );
        }
        if self.status != Status::Ready {
            panic!("payment {:?} is not ready", self.id);
        }
        if reservation.status != balance::ReservationStatus::Pending {
            panic!(
                "reservation {:?} is not pending for payment {:?}",
                reservation.id, self.id
            );
        }
        if self.reservation_id != Some(reservation.id) {
            panic!(
                "reservation {:?} does not match {:?} for payment {:?}",
                reservation.id, self.reservation_id, self.id
            );
        }
        let fee = self
            .fee
            .expect("fee should be set for a payment in ready state");
        // If the amount is specified in the invoice, we shouldn't pass it to the node.
        let amount = if self
            .invoice
            .parse()
            .unwrap()
            .amount_milli_satoshis()
            .is_some()
        {
            None
        } else {
            Some(self.amount)
        };
        match node.pay_invoice(&self.invoice, amount, fee).await {
            Ok(()) => {
                reservation.debit();
                self.status = Status::Succeeded {
                    timestamp: Utc::now(),
                };
                Ok(())
            }
            Err(ln::PaymentError::Unknown) => {
                log::error!(
                    "payment outcome unknown for {:?}, this might require manual intervention",
                    self.id
                );
                Err(Error::PaymentError(ln::PaymentError::Unknown))
            }
            Err(e) => {
                reservation.refund(balance);
                self.fail(&e);
                Err(Error::PaymentError(e))
            }
        }
    }

    fn fail(&mut self, e: &ln::PaymentError) {
        self.status = Status::Failed {
            reason: match e {
                ln::PaymentError::Unknown => "UNKNOWN".to_owned(),
                ln::PaymentError::InvoiceExpired => "INVOICE_EXPIRED".to_owned(),
                ln::PaymentError::InvoiceAlreadyPaid => "INVOICE_ALREADY_PAID".to_owned(),
                ln::PaymentError::TimedOut => "TIMED_OUT".to_owned(),
                ln::PaymentError::NoRouteFound => "NO_ROUTE_FOUND".to_owned(),
                ln::PaymentError::InvalidPaymentDetails(_) => "INVALID_PAYMENT_DETAILS".to_owned(),
                ln::PaymentError::InsufficientLiquidity => "INSUFFICIENT_LIQUIDITY".to_owned(),
            },
            timestamp: Utc::now(),
        };
    }
}
