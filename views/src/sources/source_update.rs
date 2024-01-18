use serde::Deserialize;
use crate::api::RouteType;
use crate::sources::SourceType;

#[derive(Deserialize)]
pub struct SourceUpdateParams {
    pub route_type: RouteType,
    pub media_id: i32,
    pub source_id: i32
}

#[derive(Deserialize)]
pub struct SourceUpdate {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub r#type: SourceType
}