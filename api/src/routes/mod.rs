//! Add top-level routes as submodules here.

use crate::{
    error::{self, JsonError},
    state::RocketState,
};
use app::QueryRange;
use rocket::{Build, FromForm, Rocket};
use rocket_okapi::{
    openapi_get_routes,
    swagger_ui::{make_swagger_ui, DefaultModelRendering, SwaggerUIConfig},
};
use schemars::JsonSchema;
use serde::Serialize;

mod deposits;
mod invoices;
mod payments;
mod user;
mod withdrawals;

const MIN_LIMIT: i64 = 1;
const MAX_LIMIT: i64 = 250;

#[derive(FromForm, JsonSchema)]
struct Range {
    limit: Option<String>,
    offset: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RangeError {
    /// Invalid limit.
    InvalidLimit,
    /// Invalid offset.
    InvalidOffset,
}

impl Range {
    fn query_range(self) -> Result<QueryRange, JsonError<RangeError>> {
        Ok(QueryRange {
            limit: Self::parse_limit(self.limit)?,
            offset: Self::parse_offset(self.offset)?,
        })
    }

    fn parse_limit(s: Option<String>) -> Result<i64, JsonError<RangeError>> {
        let limit: i64 = s.unwrap_or_else(|| "100".to_owned()).parse().map_err(|_| {
            error::bad_request(RangeError::InvalidLimit, "limit is not a number".to_owned())
        })?;
        if limit < MIN_LIMIT {
            Err(error::bad_request(
                RangeError::InvalidLimit,
                format!("limit must be at least {}", MIN_LIMIT),
            ))
        } else if limit > MAX_LIMIT {
            Err(error::bad_request(
                RangeError::InvalidLimit,
                format!("limit can be at most {}", MAX_LIMIT),
            ))
        } else {
            Ok(limit)
        }
    }

    fn parse_offset(s: Option<String>) -> Result<i64, JsonError<RangeError>> {
        let offset = s.unwrap_or_else(|| "0".to_owned()).parse().map_err(|_| {
            error::bad_request(
                RangeError::InvalidOffset,
                "offset is not a number".to_owned(),
            )
        })?;
        if offset < 0 {
            Err(error::bad_request(
                RangeError::InvalidOffset,
                "offset must be positive".to_owned(),
            ))
        } else {
            Ok(offset)
        }
    }
}

const VERSION: &str = "/v0";

pub fn register(rocket: Rocket<Build>, state: RocketState) -> Rocket<Build> {
    let rocket = rocket.manage(state);
    let rocket = rocket.mount(
        VERSION,
        openapi_get_routes![
            user::get,
            deposits::post_address,
            deposits::list_addresses,
            deposits::get_address,
            deposits::list_deposits,
            deposits::get_deposit,
            invoices::post,
            invoices::list,
            invoices::get,
            payments::post,
            payments::list,
            payments::get,
            withdrawals::post,
            withdrawals::list,
            withdrawals::get,
        ],
    );
    mount_swagger(rocket)
}

pub fn mount_swagger(rocket: Rocket<Build>) -> Rocket<Build> {
    rocket.mount(
        format!("{}/swagger", VERSION),
        make_swagger_ui(&SwaggerUIConfig {
            url: "../openapi.json".to_owned(),
            default_model_rendering: DefaultModelRendering::Model,
            show_extensions: true,
            ..Default::default()
        }),
    )
}
