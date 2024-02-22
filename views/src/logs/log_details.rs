use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct LogDetailsParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub log_id: i32
}