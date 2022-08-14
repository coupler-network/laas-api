//! Implements checking for send/receive limits.

use crate::btc;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("amount too low")]
    AmountTooLow,
    #[error("amount too high")]
    AmountTooHigh,
    #[error("daily limit exceeded")]
    DailyLimitExceeded,
}

#[derive(Debug)]
pub struct CashLimits {
    pub min: btc::MilliSats,
    pub max: btc::MilliSats,
    pub daily: btc::MilliSats,
}

#[derive(Debug)]
pub(crate) struct Amounts {
    /// Send or receive amount.
    pub amount: btc::MilliSats,
    /// Total amount sent/received today.
    pub daily_total: btc::MilliSats,
}

impl CashLimits {
    /// Returns an error if any limits are violated.
    pub(crate) fn check(
        &self,
        Amounts {
            amount,
            daily_total,
        }: Amounts,
    ) -> Result<(), Error> {
        if amount < self.min {
            Err(Error::AmountTooLow)
        } else if amount > self.max {
            Err(Error::AmountTooHigh)
        } else if daily_total + amount > self.daily {
            Err(Error::DailyLimitExceeded)
        } else {
            Ok(())
        }
    }
}
