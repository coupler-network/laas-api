use std::future::Future;

use app::{database::Database, user};
use okapi::openapi3::{Object, SecurityRequirement, SecurityScheme, SecuritySchemeData};
use rocket::{
    async_trait,
    http::Status,
    request::{FromRequest, Outcome},
    Request,
};
use rocket_okapi::{
    gen::OpenApiGenerator,
    request::{OpenApiFromRequest, RequestHeaderInput},
};
use thiserror::Error;

use crate::state::RocketState;

pub struct SpendGuard(app::auth::SpendGrant);

impl SpendGuard {
    pub fn grant(&self) -> &app::auth::SpendGrant {
        &self.0
    }
}

pub struct ReceiveGuard(app::auth::ReceiveGrant);

impl ReceiveGuard {
    pub fn grant(&self) -> &app::auth::ReceiveGrant {
        &self.0
    }
}

pub struct ReadGuard(app::auth::ReadGrant);

impl ReadGuard {
    pub fn grant(&self) -> &app::auth::ReadGrant {
        &self.0
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("access denied")]
    AccessDenied(#[from] app::auth::AccessDenied),
    #[error("rate limit exceeded")]
    RateLimited,
}

const TOKEN_HEADER: &str = "X-Auth-Token";

#[async_trait]
impl<'r> FromRequest<'r> for SpendGuard {
    type Error = Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        guard_impl(req, app::auth::get_spend_grant, Self).await
    }
}

#[async_trait]
impl<'r> FromRequest<'r> for ReceiveGuard {
    type Error = Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        guard_impl(req, app::auth::get_receive_grant, Self).await
    }
}

#[async_trait]
impl<'r> FromRequest<'r> for ReadGuard {
    type Error = Error;

    async fn from_request(req: &'r Request<'_>) -> Outcome<Self, Self::Error> {
        guard_impl(req, app::auth::get_read_grant, Self).await
    }
}

impl<'a> OpenApiFromRequest<'a> for SpendGuard {
    fn from_request_input(
        _gen: &mut OpenApiGenerator,
        _name: String,
        _required: bool,
    ) -> rocket_okapi::Result<RequestHeaderInput> {
        Ok(openapi_auth())
    }
}

impl<'a> OpenApiFromRequest<'a> for ReceiveGuard {
    fn from_request_input(
        _: &mut OpenApiGenerator,
        _: String,
        _: bool,
    ) -> rocket_okapi::Result<RequestHeaderInput> {
        Ok(openapi_auth())
    }
}

impl<'a> OpenApiFromRequest<'a> for ReadGuard {
    fn from_request_input(
        _: &mut OpenApiGenerator,
        _: String,
        _: bool,
    ) -> rocket_okapi::Result<RequestHeaderInput> {
        Ok(openapi_auth())
    }
}

async fn guard_impl<
    'a,
    'b,
    G: AnyGrant,
    F: Future<Output = Result<G, app::auth::AccessDenied>> + 'a,
    R,
>(
    req: &'a Request<'b>,
    get_grant: impl FnOnce(&'a Database, &'a str) -> F,
    create_guard: impl FnOnce(G) -> R,
) -> Outcome<R, Error> {
    match req.headers().get_one(TOKEN_HEADER) {
        Some(token) => {
            let state = req.rocket().state::<RocketState>().unwrap();
            match get_grant(&state.db, token).await {
                Ok(grant) => {
                    if state.rate_limit.limit(grant.user_id()) {
                        log::info!("rate limiting user {:?}", grant.user_id());
                        Outcome::Failure((Status::TooManyRequests, Error::RateLimited))
                    } else {
                        Outcome::Success(create_guard(grant))
                    }
                }
                Err(e) => Outcome::Failure((Status::Forbidden, e.into())),
            }
        }
        None => Outcome::Failure((Status::Forbidden, app::auth::AccessDenied.into())),
    }
}

/// Helper trait implemented for all grant types.
trait AnyGrant {
    /// Every grant applies to a user.
    fn user_id(&self) -> user::Id;
}

impl AnyGrant for app::auth::SpendGrant {
    fn user_id(&self) -> user::Id {
        self.user_id
    }
}

impl AnyGrant for app::auth::ReceiveGrant {
    fn user_id(&self) -> user::Id {
        self.user_id
    }
}

impl AnyGrant for app::auth::ReadGrant {
    fn user_id(&self) -> user::Id {
        self.user_id
    }
}

fn openapi_auth() -> RequestHeaderInput {
    let security_scheme = SecurityScheme {
        description: Some(format!(
            "Requires an API key to access: \"{}\".",
            TOKEN_HEADER
        )),
        data: SecuritySchemeData::ApiKey {
            name: TOKEN_HEADER.to_owned(),
            location: "header".to_owned(),
        },
        extensions: Object::default(),
    };
    let mut security_req = SecurityRequirement::new();
    security_req.insert(TOKEN_HEADER.to_owned(), Vec::new());
    RequestHeaderInput::Security(TOKEN_HEADER.to_owned(), security_scheme, security_req)
}
