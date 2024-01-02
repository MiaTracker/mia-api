use serde::Deserialize;
use crate::routing::RouteType;

#[derive(Deserialize)]
pub struct GenreCreate {
    pub name: String
}

#[derive(Deserialize)]
pub struct GenreCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}