use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use crate::media::MediaType;

#[derive(Deserialize, IntoParams)]
pub struct PageReq {
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub limit: Option<u64>
}

#[derive(Serialize, ToSchema)]
pub struct MediaIndex {
    pub id: i32,
    pub r#type: MediaType,
    pub poster_path: Option<String>,
    pub stars: Option<f32>,
    pub title: String
}