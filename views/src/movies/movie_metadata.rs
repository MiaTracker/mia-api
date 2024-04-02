use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct MovieMetadata {
    pub id: i32,
    pub homepage: Option<String>,
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub original_language: Option<String>,
    pub release_date: Option<chrono::NaiveDate>,
    pub runtime: Option<i32>,
    pub status: Option<String>
}