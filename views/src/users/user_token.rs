use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct UserToken {
    pub token: String,
    pub expiry_date: DateTime<Utc>
}