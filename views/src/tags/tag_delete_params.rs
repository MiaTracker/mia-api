use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct TagDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub tag_id: i32
}