use serde::Deserialize;
use utoipa::IntoParams;
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct SourceDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub source_id: i32
}