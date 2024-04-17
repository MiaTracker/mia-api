use std::fmt;
use std::fmt::Formatter;

use axum_core::response::{IntoResponse, Response};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub struct ApiErr {
    pub errors: Vec<ApiErrView>,
    pub status: StatusCode,
}

#[derive(Serialize, Deserialize, ToSchema)]
#[schema(as = Error)]
pub struct ApiErrView {
    pub key: String,
    pub debug_message: String
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ErrorKey {
    InternalServerError,
    NoAuthenticationTokenProvided,
    InvalidAuthenticationToken,
    MasterdataOutdated,
    InsufficientPermissions,
    InternalClientError
}

impl fmt::Display for ErrorKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        let json = match serde_json::to_string(&self.errors) {
            Ok(str) => { str }
            Err(err) => { format!("Error serializing ApiErr: {}", err.to_string()) }
        };
        (self.status, json).into_response()
    }
}