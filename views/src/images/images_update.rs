use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct ImagesUpdate {
    pub backdrop_path: Option<String>,
    pub poster_path: Option<String>
}

#[derive(Deserialize, ToSchema)]
pub struct BackdropUpdate {
    pub path: String,
}

#[derive(Deserialize, ToSchema)]
pub struct PosterUpdate {
    pub path: String
}