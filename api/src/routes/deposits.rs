use super::{Range, RangeError};
use crate::{access, error::JsonResult, state::RocketState};
use app::btc;
use chrono::{DateTime, Utc};
use rocket::{get, post, serde::json::Json, State};
use rocket_okapi::openapi;
use schemars::JsonSchema;
use serde::Serialize;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct AddressModel {
    /// The BTC address used to deposit funds into your balance.
    address: String,
    /// Address creation time.
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct AddressResponse {
    deposit_address: AddressModel,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct AddressesResponse {
    deposit_addresses: Vec<AddressModel>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct DepositModel {
    /// Unique deposit identifier.
    id: Uuid,
    /// The BTC address used for this deposit.
    address: String,
    /// TXID of the BTC transaction funding this deposit.
    txid: String,
    /// Amount of satoshis deposited.
    amount_sats: i64,
    /// True if the related BTC transaction was confirmed.
    is_confirmed: bool,
    /// Deposit creation time.
    created_at: DateTime<Utc>,
    /// Deposit confirmation time, if the deposit was confirmed.
    confirmed_at: Option<DateTime<Utc>>,
}

impl DepositModel {
    fn from_entity(deposit: &app::deposit::Deposit) -> Self {
        Self {
            id: deposit.id.0,
            address: deposit.tx_out.address.to_string(),
            txid: deposit.tx_out.tx.id.to_string(),
            amount_sats: deposit.tx_out.amount.0,
            is_confirmed: deposit.is_confirmed(),
            created_at: deposit.created,
            confirmed_at: deposit.confirmed,
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct DepositResponse {
    deposit: DepositModel,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct DepositsResponse {
    deposits: Vec<DepositModel>,
}

/// Create a new deposit address. You can use your BTC wallet to pay to this address and
/// deposit funds into your coupler.network account.
#[openapi(tag = "Deposit Addresses")]
#[post("/deposits/addresses")]
pub(super) async fn post_address(
    state: &State<RocketState>,
    guard: access::ReceiveGuard,
) -> Json<AddressResponse> {
    let address = app::deposit::create_address(
        guard.grant(),
        &state.db,
        state.lightning.create_node().await,
    )
    .await;
    Json(AddressResponse {
        deposit_address: AddressModel {
            address: address.address.to_string(),
            created_at: address.created,
        },
    })
}

/// List deposit addresses.
#[openapi(tag = "Deposit Addresses")]
#[get("/deposits/addresses?<range..>")]
pub(super) async fn list_addresses(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    range: Range,
) -> JsonResult<AddressesResponse, RangeError> {
    let addresses =
        app::deposit::get_addresses(guard.grant(), &state.db, range.query_range()?).await;
    Ok(Json(AddressesResponse {
        deposit_addresses: addresses
            .into_iter()
            .map(|address| AddressModel {
                address: address.address.to_string(),
                created_at: address.created,
            })
            .collect(),
    }))
}

/// Get deposit address details.
#[openapi(tag = "Deposit Addresses")]
#[get("/deposits/addresses/<address>")]
pub(super) async fn get_address(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    address: &str,
) -> Option<Json<AddressResponse>> {
    match btc::Address::from_str(address) {
        Ok(address) => {
            let address = app::deposit::get_address(guard.grant(), &state.db, &address).await;
            address.map(|address| {
                Json(AddressResponse {
                    deposit_address: AddressModel {
                        address: address.address.to_string(),
                        created_at: address.created,
                    },
                })
            })
        }
        Err(_) => None,
    }
}

/// List deposits.
#[openapi(tag = "Deposits")]
#[get("/deposits?<range..>")]
pub(super) async fn list_deposits(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    range: Range,
) -> JsonResult<DepositsResponse, RangeError> {
    let deposits = app::deposit::list(guard.grant(), &state.db, range.query_range()?)
        .await
        .iter()
        .map(DepositModel::from_entity)
        .collect();
    Ok(Json(DepositsResponse { deposits }))
}

/// Get deposit details.
#[openapi(tag = "Deposits")]
#[get("/deposits/<deposit_id>")]
pub(super) async fn get_deposit(
    state: &State<RocketState>,
    guard: access::ReadGuard,
    deposit_id: String,
) -> Option<Json<DepositResponse>> {
    match Uuid::from_str(&deposit_id) {
        Ok(deposit_id) => app::deposit::get(guard.grant(), &state.db, app::deposit::Id(deposit_id))
            .await
            .map(|deposit| {
                Json(DepositResponse {
                    deposit: DepositModel::from_entity(&deposit),
                })
            }),
        Err(_) => None,
    }
}
