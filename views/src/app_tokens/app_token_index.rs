use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct AppTokenIndex {
    pub name: String,
    #[schema(value_type = String, format = DateTime)]
    pub generated: DateTime<Utc>
}