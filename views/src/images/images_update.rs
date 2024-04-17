use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct ImagesUpdate {
    pub backdrop_path: Option<String>,
    pub poster_path: Option<String>
}