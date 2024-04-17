use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use crate::api::RouteType;
use crate::sources::SourceType;

#[derive(Deserialize, IntoParams)]
pub struct SourceCreateParams {
    pub route_type: RouteType,
    pub media_id: i32
}

#[derive(Deserialize, ToSchema)]
pub struct SourceCreate {
    pub name: String,
    pub url: String,
    pub r#type: SourceType
}