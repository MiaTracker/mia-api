use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use crate::api::RouteType;

#[derive(Deserialize, IntoParams)]
pub struct TagCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize, ToSchema)]
pub struct TagCreate {
    pub name: String
}