use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct SeriesMetadata {
    pub id: i32,
    pub backdrop_path: Option<String>,
    pub homepage: Option<String>,
    pub tmdb_id: Option<i32>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<String>,
    pub first_air_date: Option<chrono::NaiveDate>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub status: Option<String>,
    pub r#type: Option<String>,
}