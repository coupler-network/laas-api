use futures::FutureExt;
use std::{future::Future, panic::AssertUnwindSafe};

pub mod auth;
mod balance;
pub mod btc;
pub mod cash_limits;
mod chain;
mod concurrency;
pub mod database;
pub mod deposit;
mod hex;
pub mod invoice;
pub mod ln;
pub mod payment;
pub mod seconds;
pub mod user;
pub mod withdrawal;
mod worker;

pub use cash_limits::CashLimits;

#[derive(Debug, Clone, Copy)]
pub struct QueryRange {
    pub limit: i64,
    pub offset: i64,
}

async fn swallow_panic(f: impl Future<Output = ()>) {
    let _ = AssertUnwindSafe(f).catch_unwind().await;
}
