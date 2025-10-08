use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ImagesConfiguration {
    pub base_url: String,
    pub secure_base_url: String,
    pub backdrop_sizes: Vec<ImageSize>,
    pub logo_sizes: Vec<ImageSize>,
    pub poster_sizes: Vec<ImageSize>,
    pub profile_sizes: Vec<ImageSize>,
    pub still_sizes: Vec<ImageSize>
}

#[derive(Serialize, ToSchema)]
pub struct ImageSize {
    pub size: Option<i32>,
    pub dimension: ImageSizeDimension,
    pub slug: String
}

#[derive(Serialize, ToSchema)]
pub enum ImageSizeDimension {
    #[serde(rename = "width")]
    Width,
    #[serde(rename = "height")]
    Height
}