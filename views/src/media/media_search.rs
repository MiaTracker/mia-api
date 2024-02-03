use serde::{Deserialize, Serialize};
use crate::media::{MediaIndex, MediaType};

#[derive(Deserialize)]
pub struct MediaSearchQueryParams {
    pub query: String,
    #[serde(default)]
    pub committed: bool
}

#[derive(Serialize)]
pub struct SearchResults {
    pub indexes: Vec<MediaIndex>,
    pub external: Vec<ExternalIndex>
}

#[derive(Serialize)]
pub struct ExternalIndex {
    pub external_id: i32,
    pub r#type: MediaType,
    pub poster_path: Option<String>,
    pub title: String
}