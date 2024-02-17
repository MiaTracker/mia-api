use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Serialize)]
pub struct AppTokenIndex {
    pub name: String,
    pub generated: DateTime<Utc>
}