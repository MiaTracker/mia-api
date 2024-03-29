use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct SourceDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub source_id: i32
}