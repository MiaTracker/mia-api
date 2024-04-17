use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct UserToken {
    pub token: String,
    #[schema(value_type = String, format = DateTime)]
    pub expiry_date: DateTime<Utc>,
    pub admin: bool
}