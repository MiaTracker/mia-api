use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Source {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub r#type: SourceType
}

#[derive(Serialize, Deserialize, ToSchema)]
pub enum SourceType {
    #[serde(rename = "torrent")]
    Torrent,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "jellyfin")]
    Jellyfin
}