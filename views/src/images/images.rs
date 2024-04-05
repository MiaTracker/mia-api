use serde::Serialize;

#[derive(Serialize)]
pub struct Images {
    pub backdrops: Vec<Image>,
    pub posters: Vec<Image>
}

#[derive(Serialize)]
pub struct Image {
    pub language: Option<String>,
    pub width: i32,
    pub height: i32,
    pub file_path: String,
    pub current: bool
}