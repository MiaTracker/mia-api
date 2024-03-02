use serde::{Deserialize, Serialize};
use crate::media::{MediaIndex, MediaType};

#[derive(Deserialize)]
pub struct SearchParams {
    pub committed: bool
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default)]
    pub genres: Option<Vec<String>>,
    #[serde(default)]
    pub only_watched: bool,
    #[serde(default)]
    pub min_stars: Option<f32>,
}

#[derive(Serialize)]
pub struct SearchResults {
    pub indexes: Vec<MediaIndex>,
    pub external: Vec<ExternalIndex>,
    pub query_valid: bool
}

#[derive(Serialize)]
pub struct ExternalIndex {
    pub external_id: i32,
    pub r#type: MediaType,
    pub poster_path: Option<String>,
    pub title: String
}