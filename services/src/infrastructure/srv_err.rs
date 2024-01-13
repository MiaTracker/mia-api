use http::StatusCode;
use sea_orm::DbErr;
use views::api::{ApiErr, ApiErrView, ErrorKey};
use crate::infrastructure::rule_violation::RuleViolation;

pub enum SrvErr {
    DB(DbErr),
    RuleViolation(Vec<RuleViolation>),
    NotFound,
    Internal(String),
    Integration(String),
    Unauthorized,
    MasterdataOutdated,
    BadRequest(String)
}

impl From<DbErr> for SrvErr {
    fn from(value: DbErr) -> Self {
        SrvErr::DB(value)
    }
}

impl From<integrations::infrastructure::Error> for SrvErr {
    fn from(value: integrations::infrastructure::Error) -> Self {
        SrvErr::Integration(value.message)
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
        }
    }
}