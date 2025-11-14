use std::fmt::{Display, Formatter};
use std::io;
use std::io::Error;
use http::StatusCode;
use sea_orm::DbErr;
use log::error;
use views::api::{ApiErr, ApiErrView, ErrorKey};
use crate::infrastructure::rule_violation::RuleViolation;

pub enum SrvErr {
    DB(DbErr),
    IO(io::Error),
    RuleViolation(Vec<RuleViolation>),
    NotFound,
    Internal(String),
    Integration(String),
    Unauthorized,
    MasterdataOutdated,
    BadRequest(String),
}

impl From<DbErr> for SrvErr {
    fn from(value: DbErr) -> Self {
        error!("DB error: {}", value.to_string());
        SrvErr::DB(value)
    }
}

impl From<integrations::infrastructure::Error> for SrvErr {
    fn from(value: integrations::infrastructure::Error) -> Self {
        error!("Integration error: {}{}", value.status_code.map_or("".to_string(), |c| format!("{} - ", c.as_str())), value.message);
        SrvErr::Integration(value.message)
    }
}

impl From<io::Error> for SrvErr {
    fn from(value: Error) -> Self {
        error!("IO error: {}", value.to_string());
        SrvErr::IO(value)
    }
}

impl Into<ApiErr> for &SrvErr {
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
            SrvErr::NotFound => {
                ApiErr {
                    errors: vec![],
                    status: StatusCode::NOT_FOUND,
                }
            }
            SrvErr::Unauthorized => {
                ApiErr {
                    errors: vec![],
                    status: StatusCode::UNAUTHORIZED
                }
            }
            SrvErr::MasterdataOutdated => {
                ApiErr {
                    errors: vec![
                        ApiErrView {
                            key: ErrorKey::MasterdataOutdated.to_string(),
                            debug_message: "Masterdata is outdated, refresh is required.".to_string(),
                        }
                    ],
                    status: StatusCode::UNPROCESSABLE_ENTITY,
                }
            }
            SrvErr::BadRequest(message) => {
                ApiErr {
                    errors: vec![
                        ApiErrView {
                            key: ErrorKey::InternalClientError.to_string(),
                            debug_message: message.clone()
                        }
                    ],
                    status: StatusCode::BAD_REQUEST
                }
            }
            SrvErr::IO(io) => {
                ApiErr {
                    errors: vec![
                        ApiErrView {
                            key: ErrorKey::InternalServerError.to_string(),
                            debug_message: io.to_string()
                        },
                    ],
                    status: StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        }
    }
}

impl Display for SrvErr {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SrvErr::DB(err) => { write!(f, "Database error: {}", err) }
            SrvErr::IO(err) => { write!(f, "IO error: {}", err) }
            SrvErr::RuleViolation(violations) => {
                for violation in violations {
                    write!(f, "Rule violation: {}", violation)?
                }
                Ok(())
            }
            SrvErr::NotFound => { write!(f, "Not found") }
            SrvErr::Internal(err) => { write!(f, "Internal error: {}", err)}
            SrvErr::Integration(err) => { write!(f, "Integration error: {}", err) }
            SrvErr::Unauthorized => { write!(f, "Unauthorized") }
            SrvErr::MasterdataOutdated => { write!(f, "Masterdata is outdated") }
            SrvErr::BadRequest(err) => { write!(f, "Bad request: {}", err) }
        }
    }
}