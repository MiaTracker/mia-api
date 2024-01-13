use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct GenreDeleteParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub genre_id: i32
}