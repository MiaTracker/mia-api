use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ImageCandidates {
    pub backdrops: Vec<ImageCandidate>,
    pub posters: Vec<ImageCandidate>
}

#[derive(Serialize, ToSchema)]
pub struct ImageCandidate {
    pub language: Option<String>,
    pub original_width: i32,
    pub original_height: i32,
    pub path: String,
    pub sizes: Vec<ImageSize>,
    pub current: bool,
    pub source: ImageSource
}

#[derive(Serialize, ToSchema)]
pub struct Image {
    pub path: String,
    pub sizes: Vec<ImageSize>
}

#[derive(Serialize, ToSchema)]
pub struct ImageSize {
    pub slug: String,
    pub width: i32,
    pub height: i32
}

#[derive(Serialize, Deserialize, ToSchema)]
pub enum ImageSource {
    #[serde(rename = "internal")]
    Internal,
    #[serde(rename = "tmdb")]
    TMDB
}