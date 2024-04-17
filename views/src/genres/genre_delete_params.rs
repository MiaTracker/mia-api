use serde::Deserialize;
use utoipa::IntoParams;
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct GenreDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub genre_id: i32
}