use serde::Deserialize;
use crate::tmdb::views::{Genre, Languages, ProductionCompany, ProductionCountry};

#[derive(Deserialize, Clone)]
pub struct SeriesDetails {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub created_by: Option<Vec<SeriesCreator>>,
    pub episode_run_time: Option<Vec<i32>>,
    pub first_air_date: Option<String>,
    pub genres: Vec<Genre>,
    pub homepage: Option<String>,
    pub id: i32,
    pub in_production: Option<bool>,
    pub languages: Option<Vec<String>>,
    pub last_air_date: Option<String>,
    pub last_episode_to_air: Option<SeriesEpisode>,
    pub name: String,
    pub next_episode_to_air: Option<SeriesEpisode>,
    pub networks: Option<Vec<SeriesNetwork>>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub origin_country: Option<Vec<String>>,
    pub original_language: Option<String>,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub popularity: f64,
    pub poster_path: Option<String>,
    pub production_companies: Option<Vec<ProductionCompany>>,
    pub production_countries: Option<Vec<ProductionCountry>>,
    pub seasons: Option<Vec<Season>>,
    pub spoken_languages: Option<Vec<Languages>>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub r#type: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>
}

#[derive(Deserialize, Clone)]
pub struct SeriesCreator {
    pub id: i32,
    pub credit_id: Option<String>,
    pub name: String,
    pub gender: Option<i32>,
    pub profile_path: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct SeriesEpisode {
    pub id: i32,
    pub name: String,
    pub overview: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>,
    pub air_date: Option<String>,
    pub episode_number: Option<i32>,
    pub production_code: Option<String>,
    pub runtime: Option<i32>,
    pub season_number: Option<i32>,
    pub show_id: Option<i32>,
    pub still_path: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct SeriesNetwork {
    pub id: i32,
    pub logo_path: Option<String>,
    pub name: String,
    pub origin_country: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct Season {
    pub air_date: Option<String>,
    pub episode_count: Option<i32>,
    pub id: i32,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub season_number: Option<i32>,
    pub vote_average: Option<f32>
}