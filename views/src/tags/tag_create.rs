use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct TagCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize)]
pub struct TagCreate {
    pub name: String
}