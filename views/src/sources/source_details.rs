use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct SourceDetailsParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub source_id: i32
}