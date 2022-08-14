use app::{database::Database, ln::Lightning};

use crate::rate_limit::RateLimit;

pub struct CashLimits {
    pub payment_limits: app::CashLimits,
    pub invoice_limits: app::CashLimits,
}

pub struct RocketState {
    pub db: Database,
    pub lightning: Lightning,
    pub cash_limits: CashLimits,
    pub rate_limit: RateLimit,
}
