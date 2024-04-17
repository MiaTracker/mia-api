use chrono::NaiveDate;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct LogCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize, ToSchema)]
pub struct LogCreate {
    #[schema(value_type = String, format = Date)]
    pub date: NaiveDate,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>,
    pub source: String
}