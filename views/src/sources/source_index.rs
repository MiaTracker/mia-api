use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct SourceIndexParams {
    pub route_type: RouteType,
    pub media_id: i32
}