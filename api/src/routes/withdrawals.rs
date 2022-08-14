use super::{Range, RangeError};
use crate::error::JsonResult;
use crate::state::RocketState;
use crate::{access, error};
use app::{btc, withdrawal};
use chrono::{DateTime, Utc};
use rocket::{get, post, serde::json::Json, State};
use rocket_okapi::openapi;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct WithdrawalRequest {
    /// The address to withdraw the funds into. A BTC transaction will be broadcast to this
    /// address as part of the withdrawal process.
    address: String,
    /// The balance you wish to withdraw, in satoshis.
    amount_sats: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
struct WithdrawalModel {
    /// Unique withdrawal identifier.
    id: Uuid,
    /// The BTC address that funds were sent to.
    address: String,
    /// Fees paid as part of this withdrawal.
    fee_sats: i64,
    /// Amount of funds withdrawn, in satoshis.
    amount_sats: i64,
    /// Withdrawal creation time.
    created_at: DateTime<Utc>,
    /// BTC transaction ID for this withdrawal, if the transaction has been broadcast.
    txid: Option<String>,
    /// Confirmation time of the related BTC transaction, if it's been confirmed.
    confirmed_at: Option<DateTime<Utc>>,
    /// True if the related BTC transaction has been confirmed.
    is_confirmed: bool,
}

impl WithdrawalModel {
    fn from_entity(withdrawal: &app::withdrawal::Withdrawal) -> Self {
        Self {
            id: withdrawal.id.0,
            address: withdrawal.address.to_string(),
            fee_sats: withdrawal.fee.0,
            amount_sats: withdrawal.amount.0,
            created_at: withdrawal.created,
            txid: withdrawal
                .tx_out
                .as_ref()
                .map(|tx_out| tx_out.tx.id.to_string()),
            confirmed_at: withdrawal.confirmed,
            is_confirmed: withdrawal.is_confirmed(),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct WithdrawalResponse {
    withdrawal: WithdrawalModel,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct WithdrawalsResponse {
    withdrawals: Vec<WithdrawalModel>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Error {
    /// Unexpected error, please contact support.
    Unknown,
    /// Insufficient balance to complete the withdrawal. Note that the balance must cover both the
    /// withdrawal amount, as well as the fees.
    InsufficientBalance,
    /// Amount must be positive.
    AmountNotPositive,
}

/// Withdraw your balance from coupler.network into a BTC address.
#[openapi(tag = "Withdrawals")]
#[post("/withdrawals", data = "<req>")]
pub(super) async fn post(
    state: &State<RocketState>,
    req: Json<WithdrawalRequest>,
    guard: access::SpendGuard,
) -> JsonResult<WithdrawalResponse, Error> {
    match app::withdrawal::start(
        guard.grant(),
        &state.db,
        state.lightning.create_node().await,
        &btc::Address::from_str(&req.address).unwrap(),
        btc::Sats(req.amount_sats),
    )
    .await
    {
        Ok(withdrawal) => Ok(Json(WithdrawalResponse {
            withdrawal: WithdrawalModel::from_entity(&withdrawal),
        })),
        Err(e) => match e {
            withdrawal::Error::InsufficientBalance(_) => Err(error::bad_request(
                Error::InsufficientBalance,
                "insufficient balance".to_owned(),
            )),
            withdrawal::Error::AmountNotPositive => Err(error::bad_request(
                Error::AmountNotPositive,
                "amount must be positive".to_owned(),
            )),
            withdrawal::Error::ConcurrencyConflict(_) => {
                Err(error::concurrency_error(Error::Unknown))
            }
        },
    }
}

/// List withdrawals.
#[openapi(tag = "Withdrawals")]
#[get("/withdrawals?<range..>")]
pub(super) async fn list(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    range: Range,
) -> JsonResult<WithdrawalsResponse, RangeError> {
    Ok(Json(WithdrawalsResponse {
        withdrawals: app::withdrawal::list(guard.grant(), &state.db, range.query_range()?)
            .await
            .iter()
            .map(WithdrawalModel::from_entity)
            .collect(),
    }))
}

/// Get withdrawal details.
#[openapi(tag = "Withdrawals")]
#[get("/withdrawals/<withdrawal_id>")]
pub(super) async fn get(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    withdrawal_id: String,
) -> Option<Json<WithdrawalResponse>> {
    match Uuid::from_str(&withdrawal_id) {
        Ok(withdrawal_id) => {
            app::withdrawal::get(guard.grant(), &state.db, app::withdrawal::Id(withdrawal_id))
                .await
                .map(|withdrawal| {
                    Json(WithdrawalResponse {
                        withdrawal: WithdrawalModel::from_entity(&withdrawal),
                    })
                })
        }
        Err(_) => None,
    }
}
