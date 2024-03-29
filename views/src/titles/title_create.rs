use serde::Deserialize;
use crate::api::RouteType;

#[derive(Deserialize)]
pub struct TitleCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize)]
pub struct TitleCreate {
    pub name: String
}