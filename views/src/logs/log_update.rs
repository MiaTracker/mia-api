use chrono::NaiveDate;
use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct LogUpdateParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub log_id: i32
}

#[derive(Deserialize)]
pub struct LogUpdate {
    pub id: i32,
    pub date: NaiveDate,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>,
    pub source: String
}