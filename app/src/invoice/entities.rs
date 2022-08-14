//! Handles the creation and settlement of users' Lightning invoices within the service.
//!
//! Create an invoice by calling [`Invoice::create`], and once it is eventually paid settle
//! the invoice via [`Invoice::settle`], which will update the user balance.

use crate::{auth, balance::Balance, btc, cash_limits, ln, seconds::Seconds, user, CashLimits};
use chrono::{DateTime, Utc};
use const_format::formatcp;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0:?}")]
    LimitsViolated(#[from] cash_limits::Error),
    #[error("amount not positive")]
    AmountNotPositive,
    #[error("invalid expiry: {0}")]
    InvalidExpiry(&'static str),
    #[error("invalid memo: {0}")]
    InvalidMemo(&'static str),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Id(pub Uuid);

#[derive(Debug)]
pub struct Invoice {
    pub id: Id,
    pub user_id: user::Id,
    pub token_id: auth::TokenId,
    pub amount: btc::MilliSats,
    pub memo: Option<String>,
    pub raw: ln::RawInvoice,
    pub created: DateTime<Utc>,
    pub settlement: Option<Settlement>,
    pub expiration: DateTime<Utc>,
}

#[derive(Debug)]
pub struct Settlement {
    pub amount: btc::MilliSats,
    pub timestamp: DateTime<Utc>,
    /// Unique index on our Lightning node, which indicates the settlement order of this invoice.
    /// When our service gets restarted, this index allows us to continue the invoice update stream
    /// from where we were before the restart.
    pub settle_index: u64,
}

const MAX_MEMO_BYTES: usize = 639;
const MAX_EXPIRY_SECONDS: i64 = 31536000;

impl Invoice {
    /// Creates a new invoice. Setting amount to None allows the payer to
    /// specify any amount they'd like to pay.
    pub(crate) async fn create(
        grant: &auth::ReceiveGrant,
        node: &mut ln::Node,
        amount: btc::MilliSats,
        memo: Option<String>,
        expiry: Seconds,
        limits: &CashLimits,
        daily_total: btc::MilliSats,
    ) -> Result<Self, Error> {
        if amount <= btc::MilliSats(0) {
            return Err(Error::AmountNotPositive);
        }
        if let Some(ref memo) = memo {
            if memo.as_bytes().len() > MAX_MEMO_BYTES {
                return Err(Error::InvalidMemo(formatcp!(
                    "memo can be up to {} bytes long",
                    MAX_MEMO_BYTES
                )));
            }
        }
        if expiry.0 <= 0 {
            return Err(Error::InvalidExpiry("expiry must be positive"));
        }
        if expiry.0 > MAX_EXPIRY_SECONDS {
            return Err(Error::InvalidExpiry(formatcp!(
                "expiry can't be more than {} seconds",
                MAX_EXPIRY_SECONDS
            )));
        }
        limits.check(cash_limits::Amounts {
            amount,
            daily_total,
        })?;
        let invoice = node.create_invoice(amount, memo.clone(), expiry).await;
        let expiration = Utc::now()
            .checked_add_signed(chrono::Duration::seconds(expiry.0))
            .unwrap();
        Ok(Self {
            id: Id(Uuid::new_v4()),
            user_id: grant.user_id,
            token_id: grant.token_id,
            amount,
            memo,
            raw: invoice,
            created: Utc::now(),
            settlement: None,
            expiration,
        })
    }

    pub fn is_settled(&self) -> bool {
        self.settlement.is_some()
    }

    pub fn is_expired(&self) -> bool {
        Utc::now() >= self.expiration
    }

    /// Settles the invoice. Credits the received funds to the user.
    pub(crate) fn settle(&mut self, balance: &mut Balance, settled_invoice: &ln::SettledInvoice) {
        if self.is_settled() {
            panic!("invoice {:?} has already been completed", self.id);
        }
        if self.user_id != balance.user_id() {
            panic!(
                "user id {:?} does not match {:?} for invoice {:?}",
                balance.user_id(),
                self.user_id,
                self.id
            );
        }
        if settled_invoice.raw != self.raw {
            panic!(
                "payment request {:?} does not match {:?} for invoice {:?}",
                settled_invoice.raw, self.raw, self.id
            );
        }
        self.settlement = Some(Settlement {
            amount: settled_invoice.amount,
            timestamp: Utc::now(),
            settle_index: settled_invoice.settle_index,
        });
        balance.credit(settled_invoice.amount);
    }
}
