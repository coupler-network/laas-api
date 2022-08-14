use std::time::Duration;

use app::btc;
use app::database::{run_migrations, seed_development_data, Database};
use app::ln::{self, Lightning};
use rocket::{launch, Build, Rocket};
use serde::Deserialize;
use url::Url;

#[derive(Debug, Deserialize)]
struct Config {
    database_url: Url,
    lnd: LndConfig,
    limits: LimitsConfig,
    rate_limit: RateLimitConfig,
}

#[derive(Debug, Deserialize)]
struct LndConfig {
    url: Url,
    macaroon_path: String,
    cert_path: String,
    first_block: u32,
}

#[derive(Debug, Deserialize)]
struct LimitsConfig {
    payment_min_sats: i64,
    payment_max_sats: i64,
    payment_daily_sats: i64,
    invoice_min_sats: i64,
    invoice_max_sats: i64,
    invoice_daily_sats: i64,
}

impl LimitsConfig {
    pub fn into_api_limits(self) -> api::CashLimits {
        api::CashLimits {
            payment_limits: app::CashLimits {
                min: btc::Sats(self.payment_min_sats).msats(),
                max: btc::Sats(self.payment_max_sats).msats(),
                daily: btc::Sats(self.payment_daily_sats).msats(),
            },
            invoice_limits: app::CashLimits {
                min: btc::Sats(self.invoice_min_sats).msats(),
                max: btc::Sats(self.invoice_max_sats).msats(),
                daily: btc::Sats(self.invoice_daily_sats).msats(),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct RateLimitConfig {
    limit: usize,
    span: Duration,
}

impl RateLimitConfig {
    fn into_rate_limit(self) -> api::RateLimit {
        api::RateLimit::new(self.limit, self.span)
    }
}

#[launch]
async fn rocket() -> _ {
    start_server().await
}

async fn start_server() -> Rocket<Build> {
    env_logger::init();

    let rocket = Rocket::build();
    let config: Config = rocket.figment().extract().unwrap();

    let db = Database::connect(config.database_url.as_str())
        .await
        .unwrap();
    let lightning = Lightning::new(ln::Config {
        endpoint: config.lnd.url,
        macaroon_path: config.lnd.macaroon_path,
        cert_path: config.lnd.cert_path,
        first_block: config.lnd.first_block,
    })
    .await;

    run_migrations(&db).await;
    #[cfg(debug_assertions)]
    seed_development_data(&db).await;

    app::withdrawal::start_workers(config.lnd.first_block, &db, &lightning).await;
    app::deposit::start_worker(config.lnd.first_block, &db, &lightning).await;
    app::invoice::start_worker(db.clone(), &lightning).await;

    api::register(
        rocket,
        db,
        lightning,
        config.limits.into_api_limits(),
        config.rate_limit.into_rate_limit(),
    )
}
