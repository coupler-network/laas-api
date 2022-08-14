//! This library contains definitions for the API layer.

use app::{database::Database, ln::Lightning};
use rocket::{Build, Rocket};
use state::RocketState;

mod access;
mod error;
mod rate_limit;
mod routes;
mod state;

pub use rate_limit::RateLimit;
pub use state::CashLimits;

pub fn register(
    rocket: Rocket<Build>,
    db: Database,
    lightning: Lightning,
    cash_limits: CashLimits,
    rate_limit: RateLimit,
) -> Rocket<Build> {
    routes::register(
        rocket,
        RocketState {
            db,
            lightning,
            cash_limits,
            rate_limit,
        },
    )
}
