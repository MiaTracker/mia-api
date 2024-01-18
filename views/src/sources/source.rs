use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct Source {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub r#type: SourceType
}

#[derive(Serialize, Deserialize)]
pub enum SourceType {
    #[serde(rename = "torrent")]
    Torrent,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "jellyfin")]
    Jellyfin
}