use crate::images::ImageSource;
use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct ImagesUpdate {
    pub backdrop_path: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_source: Option<ImageSource>,
    pub poster_source: Option<ImageSource>,
}

#[derive(Deserialize, ToSchema)]
pub struct BackdropUpdate {
    pub path: String,
    pub source: ImageSource
}

#[derive(Deserialize, ToSchema)]
pub struct PosterUpdate {
    pub path: String,
    pub source: ImageSource
}