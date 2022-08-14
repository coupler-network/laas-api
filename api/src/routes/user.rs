//! Routes for querying user information.

use rocket::{get, serde::json::Json, State};
use rocket_okapi::{openapi, JsonSchema};
use serde::Serialize;
use std::fmt::Debug;

use app::user;

use crate::{access, state::RocketState};

#[derive(Debug, Serialize, JsonSchema)]
struct UserModel {
    /// Registered user email.
    email: String,
    /// Current balance in millisatoshis.
    balance_msats: i64,
    /// Current balance in satoshis. This is the actual withdrawable balance.
    balance_sats: i64,
}

#[derive(Debug, Serialize, JsonSchema)]
pub(super) struct UserResponse {
    user: UserModel,
}

/// Get user details, such as the current balance.
#[openapi(tag = "User")]
#[get("/user")]
pub(super) async fn get(
    guard: access::ReadGuard,
    state: &State<RocketState>,
) -> Option<Json<UserResponse>> {
    user::get(guard.grant(), &state.db).await.map(|user| {
        Json(UserResponse {
            user: UserModel {
                email: user.email.0,
                balance_msats: user.balance.0,
                balance_sats: user.balance.sats_floor().0,
            },
        })
    })
}
