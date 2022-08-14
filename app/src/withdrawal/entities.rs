//! Enables withdrawal of funds from our service into an onchain address. This is the primary way
//! for users to get funds out of our service. When a user requests a withdrawal, a new
//! [`Withdrawal`] is created. Then, the [`Withdrawal::send`] method is called, broadcasting the
//! withdrawal transaction to the BTC network. Once that transaction is confirmed,
//! [`Withdrawal::confirm`] is called.

use crate::{
    auth,
    balance::{self, Balance},
    btc, concurrency, ln, user,
};
use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum Error {
    #[error("insufficient balance")]
    InsufficientBalance(#[from] balance::InsufficientBalance),
    #[error("{0:?}")]
    ConcurrencyConflict(#[from] concurrency::ConflictError),
    #[error("amount not positive")]
    AmountNotPositive,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Id(pub Uuid);

/// Represents a withdrawal of user funds from our service into an onchain address.
pub struct Withdrawal {
    pub id: Id,
    pub user_id: user::Id,
    pub token_id: auth::TokenId,
    pub reservation_id: balance::ReservationId,
    pub address: btc::Address,
    pub fee: btc::Sats,
    pub amount: btc::Sats,
    pub tx_out: Option<btc::TxOut>,
    pub created: DateTime<Utc>,
    pub confirmed: Option<DateTime<Utc>>,
}

impl Withdrawal {
    /// Starts a new withdrawal. Reserves user funds. This method will estimate and save the
    /// transaction fees, but it will not broadcast the transaction. For broadcasting, see the
    /// [`send`] method.
    pub(crate) async fn start(
        grant: &auth::SpendGrant,
        node: &mut ln::Node,
        balance: &mut Balance,
        address: btc::Address,
        amount: btc::Sats,
    ) -> Result<(Self, balance::Reservation), Error> {
        if grant.user_id != balance.user_id() {
            panic!(
                "user id {:?} does not match grant {:?} with user id {:?}",
                balance.user_id(),
                grant.token_id,
                grant.user_id
            );
        }
        if amount <= btc::Sats(0) {
            return Err(Error::AmountNotPositive);
        }
        let fee = node.estimate_fee(amount, &address).await;
        // TODO Pricing (fees). We should probably have withdrawal fees.
        // TODO There should be a minimum limit for withdrawals. This should probably be part of
        // the pricing package.
        let reservation = balance.reserve(amount.msats() + fee.msats())?;
        Ok((
            Self {
                id: Id(Uuid::new_v4()),
                token_id: grant.token_id,
                reservation_id: reservation.id,
                user_id: grant.user_id,
                amount,
                fee,
                address,
                tx_out: None,
                created: Utc::now(),
                confirmed: None,
            },
            reservation,
        ))
    }

    pub fn is_sent(&self) -> bool {
        self.tx_out.is_some()
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed.is_some()
    }

    /// Broadcasts the withdrawal transaction to the BTC network.
    pub(crate) async fn send(&mut self, node: &mut ln::Node) {
        // TODO Currently, a lock is acquired before calling this method to avoid race conditions.
        // Use PSBTs in the future.
        if self.is_sent() {
            panic!("withdrawal {:?} has already been sent", self.id);
        }
        let tx_out = match node
            .get_tx(&self.address, self.amount, &self.id.0.to_string())
            .await
        {
            Some(tx_out) => tx_out,
            None => {
                node.send_onchain(&self.address, self.amount, &self.id.0.to_string())
                    .await
            }
        };
        self.tx_out = Some(tx_out);
    }

    /// Marks the withdrawal as confirmed, and marks the user balance reservation as irrevocably
    /// debited. This method gets called when the withdrawal transaction is confirmed on the BTC
    /// network.
    pub(crate) fn confirm(&mut self, tx_out: &btc::TxOut, reservation: &mut balance::Reservation) {
        if !self.is_sent() {
            panic!("withdrawal {:?} has not been sent", self.id);
        }
        if self.is_confirmed() {
            panic!("withdrawal {:?} has already been completed", self.id);
        }
        if !tx_out.tx.is_confirmed() {
            panic!(
                "attempted to complete withdrawal {:?} with unconfirmed tx {:?}",
                self.id, tx_out.tx.id
            );
        }
        if tx_out.tx.id != self.tx_out.as_ref().unwrap().tx.id {
            panic!(
                "withdrawal {:?} with tx out {:?} is not confirmed by {:?}",
                self.id,
                self.tx_out.as_ref().unwrap().tx.id,
                tx_out.tx.id
            );
        }
        if reservation.status != balance::ReservationStatus::Pending {
            panic!(
                "reservation {:?} is not pending for withdrawal {:?}",
                reservation.id, self.id
            );
        }
        if self.reservation_id != reservation.id {
            panic!(
                "reservation {:?} does not match {:?} for withdrawal {:?}",
                reservation.id, self.reservation_id, self.id
            );
        }
        self.tx_out = Some(tx_out.clone());
        self.confirmed = Some(Utc::now());
        reservation.debit();
    }
}
