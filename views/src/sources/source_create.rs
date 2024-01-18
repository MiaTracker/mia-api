use serde::Deserialize;
use crate::api::RouteType;
use crate::sources::SourceType;

#[derive(Deserialize)]
pub struct SourceCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize)]
pub struct SourceCreate {
    pub name: String,
    pub url: String,
    pub r#type: SourceType
}