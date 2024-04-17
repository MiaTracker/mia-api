use serde::Deserialize;
use utoipa::IntoParams;
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct TagDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub tag_id: i32
}