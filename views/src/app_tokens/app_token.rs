use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct AppToken {
    pub name: String,
    pub token: String,
    pub generated: DateTime<Utc>
}