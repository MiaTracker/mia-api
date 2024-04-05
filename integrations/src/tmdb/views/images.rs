use serde::Deserialize;

#[derive(Deserialize)]
pub struct TmdbImages {
    pub id: i32,
    pub backdrops: Vec<TmdbImage>,
    pub logos: Vec<TmdbImage>,
    pub posters: Vec<TmdbImage>
}

#[derive(Deserialize)]
pub struct TmdbImage {
    pub aspect_ratio: f32,
    pub height: i32,
    pub iso_639_1: Option<String>,
    pub file_path: String,
    pub vote_average: f32,
    pub vote_count: i32,
    pub width: i32
}