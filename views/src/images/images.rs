use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Images {
    pub backdrops: Vec<Image>,
    pub posters: Vec<Image>
}

#[derive(Serialize, ToSchema)]
pub struct Image {
    pub language: Option<String>,
    pub width: i32,
    pub height: i32,
    pub file_path: String,
    pub current: bool
}