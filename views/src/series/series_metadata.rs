use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema)]
pub struct SeriesMetadata {
    pub id: i32,
    pub homepage: Option<String>,
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub original_language: Option<String>,
    #[schema(value_type = String, format = Date)]
    pub first_air_date: Option<chrono::NaiveDate>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub status: Option<String>,
    pub r#type: Option<String>,
}