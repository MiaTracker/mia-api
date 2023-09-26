use sea_orm::DbErr;
use crate::infrastructure::rule_violation::RuleViolation;

pub enum SrvErr {
    DB(DbErr),
    RuleViolation(Vec<RuleViolation>),
    Internal(String)
}