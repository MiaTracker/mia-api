use serde::Deserialize;
use utoipa::IntoParams;
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct LogDetailsParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub log_id: i32
}