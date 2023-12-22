use serde::Serialize;
use crate::media::MediaType;

#[derive(Serialize)]
pub struct MediaIndex {
    pub id: i32,
    pub r#type: MediaType,
    pub poster_path: Option<String>,
    pub stars: Option<f32>,
    pub title: String
}