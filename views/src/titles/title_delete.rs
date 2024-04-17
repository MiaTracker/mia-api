use serde::Deserialize;
use utoipa::IntoParams;
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct TitleDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub title_id: i32
}