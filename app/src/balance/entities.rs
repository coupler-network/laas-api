//! Provides facilities for operating on user balances. These are a
//! building block for other modules. There are two operations that happen to user balances: credits,
//! which increase the balance, and debits, which decrease the balance. Debits are slightly tricky
//! because precautions must be taken so that balances can't go negative due to concurrency.
//! The typical use case is to first reserve some funds from
//! the user, then execute an irrevocable action (e.g. broadcasting a BTC transaction or paying a
//! Lightning invoice), and then finally debit the reserved funds. This module provides safe debit
//! operations using the [`Reservation`] type. The reservations are used as follows:
//! - create a reservation, which debits the user balance
//! - commit the reservation to the database before moving on
//! - execute the actual irrevocable operation, as described above
//! - if something fails in an unrecoverable way, call [`Reservation::refund`], which returns the
//! funds to the user and marks the reservation as refunded
//! - if all goes correctly, call [`Reservation::debit`], which marks the reservation as final and
//! completed; the funds cannot be returned to the user after this.

use crate::btc;
use crate::user;
use chrono::DateTime;
use chrono::Utc;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
#[error("insufficient balance")]
pub struct InsufficientBalance;

/// Represents the user balance.
///
/// Notice that this struct stores the original amount as well as any updates done on the balance.
/// This allows us to write SQL queries that avoid concurrency issues - in general, a balance will only be
/// updated successfully if no other process updated the balance in between the time when we loaded it
/// and the time when we tried to update it.
#[derive(Debug, Clone, Default)]
pub struct Balance {
    user_id: user::Id,
    original_amount: btc::MilliSats,
    amount: btc::MilliSats,
}

impl Balance {
    pub fn new(user_id: user::Id, amount: btc::MilliSats) -> Self {
        Self {
            user_id,
            original_amount: amount,
            amount,
        }
    }

    pub fn user_id(&self) -> user::Id {
        self.user_id
    }

    pub fn original_amount(&self) -> btc::MilliSats {
        self.original_amount
    }

    pub fn amount(&self) -> btc::MilliSats {
        self.amount
    }

    pub fn changed(&self) -> bool {
        self.original_amount != self.amount
    }

    pub fn credit(&mut self, amount: btc::MilliSats) {
        self.amount += amount
    }

    /// Debits the user balance and creates a reservation. See [`Reservation`].
    pub fn reserve(&mut self, amount: btc::MilliSats) -> Result<Reservation, InsufficientBalance> {
        if amount > self.amount {
            return Err(InsufficientBalance);
        }
        self.amount -= amount;
        Ok(Reservation {
            id: ReservationId(Uuid::new_v4()),
            user_id: self.user_id,
            amount,
            status: ReservationStatus::Pending,
            created: Utc::now(),
        })
    }
}

/// Reservations enable safe debits, where some funds are "locked" before being irrevocably spent
/// (e.g. by broadcasting an onchain transaction). Note that the user balance is actually debited
/// when the reservation is created; after this, the funds can either be refunded to the user via
/// [`Reservation::refund`] or marked as irrevocably spent via [`Reservation::debit`]. Both of
/// these operations are final, meaning that [`Reservation::debit`] cannot be called on a refunded
/// reservation and vice versa.
#[derive(Debug)]
pub struct Reservation {
    pub id: ReservationId,
    pub user_id: user::Id,
    pub amount: btc::MilliSats,
    pub status: ReservationStatus,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReservationId(pub Uuid);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservationStatus {
    Pending,
    Debited,
    Refunded,
}

impl Reservation {
    /// Marks the reservation as finally debited, meaning that the funds have been irrevocably
    /// spent.
    pub fn debit(&mut self) {
        if self.status != ReservationStatus::Pending {
            panic!(
                "trying to debit a {:?} reservation {:?}",
                self.status, self.id
            );
        }
        self.status = ReservationStatus::Debited;
    }

    /// Credits the funds back to the user, and marks the reservation as finally refunded.
    pub fn refund(&mut self, balance: &mut Balance) {
        if self.status != ReservationStatus::Pending {
            panic!(
                "trying to debit a {:?} reservation {:?}",
                self.status, self.id
            );
        }
        self.status = ReservationStatus::Refunded;
        balance.credit(self.amount);
    }
}
