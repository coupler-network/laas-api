use super::{Range, RangeError};
use crate::{
    access,
    error::{self, JsonResult},
    state::RocketState,
};
use app::{btc, cash_limits, ln, payment};
use chrono::{DateTime, Utc};
use rocket::{get, post, serde::json::Json, State};
use rocket_okapi::openapi;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Deserialize, JsonSchema)]
pub(super) struct PaymentRequest {
    /// Invoice to pay aka payment request.
    invoice: String,
    // TODO Remove this when we remove amountless invoices
    amount_msats: Option<u64>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct PaymentModel {
    /// Unique payment identifier.
    id: Uuid,
    /// Amount paid in millisatoshis.
    amount_msats: i64,
    /// Fee paid in millisatoshis.
    fee_msats: Option<i64>,
    /// The payment invoice aka payment request.
    invoice: String,
    /// Payment creation time.
    created_at: DateTime<Utc>,
    /// Payment status.
    status: PaymentStatus,
    /// Failure reason, in case the payment failed.
    failure_reason: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PaymentStatus {
    /// Newly created payment, waiting to be sent.
    New,
    /// The payment failed.
    Failed,
    /// The payment was sent successfully.
    Succeeded,
}

impl PaymentModel {
    fn from_entity(payment: &app::payment::Payment) -> Self {
        Self {
            id: payment.id.0,
            amount_msats: payment.amount.0,
            fee_msats: payment.fee.map(|fee| fee.0),
            invoice: payment.invoice.0.clone(),
            created_at: payment.created,
            status: match payment.status {
                app::payment::Status::New | app::payment::Status::Ready => PaymentStatus::New,
                app::payment::Status::Failed { .. } => PaymentStatus::Failed,
                app::payment::Status::Succeeded { .. } => PaymentStatus::Succeeded,
            },
            failure_reason: match payment.status {
                app::payment::Status::Failed { ref reason, .. } => Some(reason.to_owned()),
                _ => None,
            },
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct PaymentResponse {
    payment: PaymentModel,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct PaymentsResponse {
    payments: Vec<PaymentModel>,
}

/// Error during payment.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Error {
    /// Unexpected error, please contact support.
    Unknown,
    /// Amount too low.
    AmountTooLow,
    /// Amount too high.
    AmountTooHigh,
    /// Daily amount exceeded.
    DailyLimitExceeded,
    /// The specified invoice was invalid.
    InvalidInvoice,
    /// Amount to pay was specified both in the invoice and in the request.
    // TODO Should we allow this in case the amounts match?
    AmountSpecifiedTwice,
    /// Amount was not specified in the invoice nor the request.
    AmountNotSpecified,
    /// Attempted to pay an expired invoice.
    InvoiceExpired,
    /// The invoice has already been paid.
    InvoiceAlreadyPaid,
    /// Payment timed out, possibly because finding a route was too difficult.
    TimedOut,
    /// Failed to route the payment.
    NoRoute,
    /// Invalid payment instructions.
    InvalidPaymentDetails,
    /// The liquidity on our Lightning nodes is running out, please contact support.
    InsufficientLiquidity,
    /// Insufficient user balance to complete the payment.
    InsufficientBalance,
}

/// Pay a Lightning invoice (aka payment request) with your coupler.network balance.
#[openapi(tag = "Payments")]
#[post("/payments", data = "<req>")]
pub(super) async fn post(
    state: &State<RocketState>,
    req: Json<PaymentRequest>,
    guard: access::SpendGuard,
) -> JsonResult<PaymentResponse, Error> {
    app::payment::send(
        guard.grant(),
        &state.db,
        state.lightning.create_node().await,
        ln::RawInvoice(req.invoice.clone()),
        req.amount_msats
            .map(|amount| btc::MilliSats(amount.try_into().unwrap())),
        &state.cash_limits.payment_limits,
    )
    .await
    .map(|payment| {
        Json(PaymentResponse {
            payment: PaymentModel::from_entity(&payment),
        })
    })
    .map_err(|e| match e {
        payment::Error::LimitsViolated(cash_limits::Error::AmountTooLow) => {
            error::bad_request(Error::AmountTooLow, "payment amount too low".to_owned())
        }
        payment::Error::LimitsViolated(cash_limits::Error::AmountTooHigh) => {
            error::bad_request(Error::AmountTooHigh, "payment amount too high".to_owned())
        }
        payment::Error::LimitsViolated(cash_limits::Error::DailyLimitExceeded) => {
            error::bad_request(
                Error::DailyLimitExceeded,
                "daily payment total exceeded".to_owned(),
            )
        }
        payment::Error::InvalidInvoice(inner) => error::bad_request(Error::InvalidInvoice, inner.0),
        payment::Error::AmountSpecifiedTwice => error::bad_request(
            Error::AmountSpecifiedTwice,
            "payment amount already specified in invoice".to_owned(),
        ),
        payment::Error::AmountNotSpecified => {
            error::bad_request(Error::AmountNotSpecified, "amount not specified".to_owned())
        }
        // TODO Log this
        payment::Error::ConcurrencyConflict(_) => error::concurrency_error(Error::Unknown),
        payment::Error::InsufficientBalance(_) => error::bad_request(
            Error::InsufficientBalance,
            "insufficient balance".to_owned(),
        ),
        payment::Error::PaymentError(inner) => match inner {
            ln::PaymentError::Unknown => error::bad_request(
                Error::Unknown,
                "payment failed for unknown reason".to_owned(),
            ),
            ln::PaymentError::InvoiceExpired => {
                error::bad_request(Error::InvoiceExpired, "invoice has expired".to_owned())
            }
            ln::PaymentError::InvoiceAlreadyPaid => error::bad_request(
                Error::InvoiceAlreadyPaid,
                "invoice has already been paid".to_owned(),
            ),
            ln::PaymentError::TimedOut => {
                error::bad_request(Error::TimedOut, "payment has failed out".to_owned())
            }
            ln::PaymentError::NoRouteFound => {
                error::bad_request(Error::NoRoute, "failed to route the payment".to_owned())
            }
            ln::PaymentError::InvalidPaymentDetails(_) => error::bad_request(
                Error::InvalidPaymentDetails,
                "invalid payment details".to_owned(),
            ),
            // TODO Log this
            ln::PaymentError::InsufficientLiquidity => error::bad_request(
                Error::InsufficientLiquidity,
                "the liquidity on our Lightning nodes is running out, please notify support"
                    .to_owned(),
            ),
        },
    })
}

/// List all payments made from your account.
#[openapi(tag = "Payments")]
#[get("/payments?<range..>")]
pub(super) async fn list(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    range: Range,
) -> JsonResult<PaymentsResponse, RangeError> {
    Ok(Json(PaymentsResponse {
        payments: app::payment::list(guard.grant(), &state.db, range.query_range()?)
            .await
            .iter()
            .map(PaymentModel::from_entity)
            .collect(),
    }))
}

/// Get payment details.
#[openapi(tag = "Payments")]
#[get("/payments/<payment_id>")]
pub(super) async fn get(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    payment_id: String,
) -> Option<Json<PaymentResponse>> {
    match Uuid::from_str(&payment_id) {
        Ok(payment_id) => app::payment::get(guard.grant(), &state.db, app::payment::Id(payment_id))
            .await
            .map(|payment| {
                Json(PaymentResponse {
                    payment: PaymentModel::from_entity(&payment),
                })
            }),
        Err(_) => None,
    }
}
