use sea_orm::DbErr;
use integrations::infrastructure::Error;
use crate::infrastructure::rule_violation::RuleViolation;

pub enum SrvErr {
    DB(DbErr),
    RuleViolation(Vec<RuleViolation>),
    NotFound,
    Internal(String),
    Integration(String),
    Unauthorized,
    MasterdataOutdated
}

impl From<DbErr> for SrvErr {
    fn from(value: DbErr) -> Self {
        SrvErr::DB(value)
    }
}

impl From<integrations::infrastructure::Error> for SrvErr {
    fn from(value: Error) -> Self {
        SrvErr::Integration(value.message)
    }
}