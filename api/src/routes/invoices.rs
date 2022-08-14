use super::{Range, RangeError};
use crate::{
    access,
    error::{self, JsonResult},
    state::RocketState,
};
use app::{btc, cash_limits, invoice, seconds::Seconds};
use chrono::{DateTime, Utc};
use rocket::{get, post, serde::json::Json, State};
use rocket_okapi::openapi;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct InvoiceRequest {
    /// Invoice description.
    memo: Option<String>,
    /// Amount to pay with this invoice.
    amount_msats: u64,
    /// Invoice expiry time. An invoice cannot be paid after it's expired.
    expiry_secs: Option<i64>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct InvoiceResponse {
    invoice: InvoiceModel,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct InvoicesResponse {
    invoices: Vec<InvoiceModel>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct InvoiceModel {
    /// The invoice, aka payment request.
    invoice: String,
    /// Invoice creation time.
    created_at: DateTime<Utc>,
    /// Invoice description.
    memo: Option<String>,
    /// Amount to pay with this invoice.
    amount_msats: i64,
    /// Invoice settle time, if the invoice has been paid.
    settled_at: Option<DateTime<Utc>>,
    /// The amount that was paid. Should match amount_msats.
    amount_paid_msats: Option<i64>,
    /// Invoice expiry time.
    expires_at: DateTime<Utc>,
    /// True if the invoice has been paid.
    is_settled: bool,
    /// True if the invoice has expired.
    is_expired: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Error {
    /// Amount too low.
    AmountTooLow,
    /// Amount too high.
    AmountTooHigh,
    /// Daily amount exceeded.
    DailyLimitExceeded,
    /// Invoice amount must be positive.
    AmountNotPositive,
    /// Expiry time must be positive.
    InvalidExpiry,
    /// Memo was too long or contained invalid characters.
    InvalidMemo,
}

impl InvoiceModel {
    fn from_entity(invoice: &app::invoice::Invoice) -> Self {
        Self {
            invoice: invoice.raw.0.clone(),
            created_at: invoice.created,
            memo: invoice.memo.clone(),
            amount_msats: invoice.amount.0,
            settled_at: invoice
                .settlement
                .as_ref()
                .map(|settlement| settlement.timestamp),
            amount_paid_msats: invoice
                .settlement
                .as_ref()
                .map(|settlement| settlement.amount.0),
            expires_at: invoice.expiration,
            is_settled: invoice.is_settled(),
            is_expired: invoice.is_expired(),
        }
    }
}

/// Create a new invoice. When this invoice is paid on the Lightning Network, the invoice amount
/// will be added to your balance.
#[openapi(tag = "Invoices")]
#[post("/invoices", data = "<req>")]
pub(super) async fn post(
    state: &State<RocketState>,
    req: Json<InvoiceRequest>,
    guard: access::ReceiveGuard,
) -> JsonResult<InvoiceResponse, Error> {
    let amount = btc::MilliSats(req.amount_msats.try_into().unwrap());
    let memo = req.memo.clone();
    let expiry = req.expiry_secs.map(Seconds);
    app::invoice::create(
        guard.grant(),
        &state.db,
        &mut state.lightning.create_node().await,
        amount,
        memo,
        expiry.unwrap_or_else(Seconds::one_hour),
        &state.cash_limits.invoice_limits,
    )
    .await
    .map(|invoice| {
        Json(InvoiceResponse {
            invoice: InvoiceModel::from_entity(&invoice),
        })
    })
    .map_err(|e| match e {
        invoice::Error::LimitsViolated(cash_limits::Error::AmountTooLow) => {
            error::bad_request(Error::AmountTooLow, "invoice amount too low".to_owned())
        }
        invoice::Error::LimitsViolated(cash_limits::Error::AmountTooHigh) => {
            error::bad_request(Error::AmountTooHigh, "invoice amount too high".to_owned())
        }
        invoice::Error::LimitsViolated(cash_limits::Error::DailyLimitExceeded) => {
            error::bad_request(
                Error::DailyLimitExceeded,
                "daily invoice total exceeded".to_owned(),
            )
        }
        invoice::Error::AmountNotPositive => error::bad_request(
            Error::AmountNotPositive,
            "amount must be positive".to_owned(),
        ),
        invoice::Error::InvalidExpiry(message) => {
            error::bad_request(Error::InvalidExpiry, message.to_owned())
        }
        invoice::Error::InvalidMemo(message) => {
            error::bad_request(Error::InvalidMemo, message.to_owned())
        }
    })
}

/// List invoices.
#[openapi(tag = "Invoices")]
#[get("/invoices?<range..>")]
pub(super) async fn list(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    range: Range,
) -> JsonResult<InvoicesResponse, RangeError> {
    Ok(Json(InvoicesResponse {
        invoices: app::invoice::list(guard.grant(), &state.db, range.query_range()?)
            .await
            .iter()
            .map(InvoiceModel::from_entity)
            .collect(),
    }))
}

/// Get invoice details.
#[openapi(tag = "Invoices")]
#[get("/invoices/<invoice_id>")]
pub(super) async fn get(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    invoice_id: String,
) -> Option<Json<InvoiceResponse>> {
    match Uuid::from_str(&invoice_id) {
        Ok(invoice_id) => app::invoice::get(guard.grant(), &state.db, app::invoice::Id(invoice_id))
            .await
            .map(|invoice| {
                Json(InvoiceResponse {
                    invoice: InvoiceModel::from_entity(&invoice),
                })
            }),
        Err(_) => None,
    }
}
