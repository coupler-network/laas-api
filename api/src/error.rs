use rocket::{http::Status, serde::json::Json};
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Serialize, JsonSchema)]
pub struct Error<E: Serialize> {
    pub error: Inner<E>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct Inner<E: Serialize> {
    pub code: u16,
    pub description: String,
    pub reason: Option<&'static str>,
    pub status: E,
}

impl<E: Serialize> Error<E> {
    fn new(http_status: Status, description: String, error: E) -> Self {
        Self {
            error: Inner {
                code: http_status.code,
                description,
                reason: http_status.reason(),
                status: error,
            },
        }
    }
}

pub type JsonError<E> = (Status, Json<Error<E>>);

pub type JsonResult<T, E> = Result<Json<T>, JsonError<E>>;

pub fn bad_request<E: Serialize>(error: E, description: String) -> JsonError<E> {
    (
        Status::BadRequest,
        Json(Error::new(Status::BadRequest, description, error)),
    )
}

pub fn internal_server_error<E: Serialize>(error: E, description: String) -> JsonError<E> {
    (
        Status::InternalServerError,
        Json(Error::new(Status::InternalServerError, description, error)),
    )
}

pub fn concurrency_error<E: Serialize>(error: E) -> JsonError<E> {
    internal_server_error(
        error,
        "a concurrency conflict could not be resolved, please contact support".to_owned(),
    )
}
