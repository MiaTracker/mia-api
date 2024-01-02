use serde::Deserialize;
use crate::routing::RouteType;

#[derive(Deserialize)]
pub struct TitleDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub title_id: i32
}