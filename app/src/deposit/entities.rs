//! Handles the logic of users depositing funds into our service.
//! The funds deposit flow goes as follows:
//! - the user generates an [`Address`], which contains an onchain address for him to send
//! BTC to
//! - the user sends any amount of BTC to the address in an onchain transaction, which causes a
//! [`Deposit`] to be created
//! - when the transaction is confirmed, the [`Deposit::confirm`] method is called, updating the
//! user balance and completing the deposit flow.

use crate::auth;
use crate::balance::Balance;
use crate::btc;
use crate::ln;
use crate::user;
use chrono::DateTime;
use chrono::Utc;
use uuid::Uuid;

/// Represents a BTC address for the user to deposit funds into. This is the primary way for users
/// to get funds into our service.
#[derive(Debug)]
pub struct Address {
    pub user_id: user::Id,
    pub token_id: auth::TokenId,
    pub address: btc::Address,
    pub created: DateTime<Utc>,
}

impl Address {
    /// Generates a new onchain deposit address.
    pub(crate) async fn generate(grant: &auth::ReceiveGrant, node: &mut ln::Node) -> Self {
        Self {
            user_id: grant.user_id,
            token_id: grant.token_id,
            address: node.generate_address().await,
            created: Utc::now(),
        }
    }

    /// Starts a new deposit of funds. This method is called whenever the user sends a new
    /// transaction to this deposit address.
    pub(crate) async fn start_deposit(&self, tx_out: &btc::TxOut) -> Deposit {
        Deposit {
            id: Id(Uuid::new_v4()),
            user_id: self.user_id,
            tx_out: tx_out.clone(),
            created: Utc::now(),
            confirmed: None,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Id(pub Uuid);

/// Corresponds to a particular BTC transaction that was deposited into an [`Address`].
#[derive(Debug)]
pub struct Deposit {
    pub id: Id,
    pub user_id: user::Id,
    pub tx_out: btc::TxOut,
    pub created: DateTime<Utc>,
    pub confirmed: Option<DateTime<Utc>>,
}

impl Deposit {
    pub fn is_confirmed(&self) -> bool {
        self.confirmed.is_some()
    }

    /// Confirms the deposit, finally updating the user balance. This method is called when the
    /// deposit transaction gets confirmed on the BTC network.
    pub(crate) fn confirm(&mut self, tx_out: &btc::TxOut, balance: &mut Balance) {
        if self.is_confirmed() {
            panic!("deposit {:?} has already been confirmed", self.id)
        }
        // TODO What about fee bumps? RBF and CPFP
        // v0l: RBF usage is very high, there should be a way to detect if the same inputs are used in
        // any other of the deposits (on confirm here) or they simply will never be confirmed.
        // v0l: For UX purposes it would be necessary to detect that the deposit was abandoned due to
        // double spends (RBF)
        // TODO Maybe just don't track unconfirmed deposits, and that solves the above?
        if tx_out.tx.id != self.tx_out.tx.id {
            panic!(
                "deposit {:?} with tx id {:?} is not confirmed by {:?}",
                self.id, self.tx_out.tx.id, tx_out.tx.id
            )
        }
        if self.user_id != balance.user_id() {
            panic!(
                "deposit {:?} user id {:?} does not match {:?}",
                self.id,
                self.user_id,
                balance.user_id()
            )
        }
        self.tx_out = tx_out.clone();
        self.confirmed = Some(Utc::now());
        balance.credit(tx_out.amount.msats());
    }
}
