use std::fmt;
use std::fmt::Formatter;
use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use services::infrastructure::SrvErr;

pub struct ApiErr {
    pub errors: Vec<ApiErrView>,
    pub status: StatusCode,
}

#[derive(Serialize)]
pub struct ApiErrView {
    pub key: String,
    pub debug_message: String
}

#[derive(Debug)]
pub enum ErrorKey {
    InternalServerError,
    NoAuthenticationTokenProvided,
    InvalidAuthenticationToken
}

impl fmt::Display for ErrorKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl Into<ApiErr> for SrvErr {
    fn into(self) -> ApiErr {
        match self {
            SrvErr::DB(db) => {
                ApiErr {
                    errors: vec![
                        ApiErrView {
                            key: ErrorKey::InternalServerError.to_string(),
                            debug_message: db.to_string()
                        },
                    ],
                    status: StatusCode::INTERNAL_SERVER_ERROR
                }
            }
            SrvErr::RuleViolation(violations) => {
                ApiErr {
                    errors: violations.iter().map(|violation| {
                        ApiErrView {
                            key: violation.to_string(),
                            debug_message: String::new()
                        }
                    }).collect(),
                    status: StatusCode::BAD_REQUEST
                }
            }
            SrvErr::Internal(internal) => {
                ApiErr {
                    errors: vec![
                        ApiErrView {
                            key: ErrorKey::InternalServerError.to_string(),
                            debug_message: internal.to_string()
                        },
                    ],
                    status: StatusCode::INTERNAL_SERVER_ERROR
                }
            }
            SrvErr::Integration(integration) => {
                ApiErr {
                    errors: vec![
                        ApiErrView {
                            key: ErrorKey::InternalServerError.to_string(),
                            debug_message: integration.to_string()
                        },
                    ],
                    status: StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
    }
}

impl IntoResponse for ApiErr {
    fn into_response(self) -> Response {
        (self.status, Json(self.errors)).into_response()
    }
}