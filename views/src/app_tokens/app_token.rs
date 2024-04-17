use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct AppToken {
    pub name: String,
    pub token: String,
    #[schema(value_type = String, format = DateTime)]
    pub generated: DateTime<Utc>
}