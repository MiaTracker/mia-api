use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct MovieMetadata {
    pub id: i32,
    pub backdrop_path: Option<String>,
    pub homepage: Option<String>,
    pub tmdb_id: Option<i32>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<String>,
    pub release_date: Option<chrono::NaiveDate>,
    pub runtime: Option<i32>,
    pub status: Option<String>
}