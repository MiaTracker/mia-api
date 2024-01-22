use chrono::NaiveDate;
use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct LogCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize)]
pub struct LogCreate {
    pub date: NaiveDate,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>,
    pub source: String
}