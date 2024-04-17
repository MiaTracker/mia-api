use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use crate::api::RouteType;

#[derive(Deserialize, ToSchema)]
pub struct GenreCreate {
    pub name: String
}

#[derive(Deserialize, IntoParams)]
pub struct GenreCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}