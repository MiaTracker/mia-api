use serde::Deserialize;
use crate::tmdb::{Languages, ProductionCompany};
use crate::tmdb::genre::Genre;
use crate::tmdb::production_country::ProductionCountry;

#[derive(Deserialize, Clone)]
pub struct SeriesDetails {
    pub adult: bool,
    pub backdrop_path: String,
    pub created_by: Vec<SeriesCreator>,
    pub episode_run_time: Vec<i32>,
    pub first_air_date: String,
    pub genres: Vec<Genre>,
    pub homepage: String,
    pub id: i32,
    pub in_production: bool,
    pub languages: Vec<String>,
    pub last_air_date: String,
    pub last_episode_to_air: Vec<SeriesEpisode>,
    pub name: String,
    pub next_episode_to_air: String,
    pub networks: Vec<SeriesNetwork>,
    pub number_of_episodes: i32,
    pub number_of_seasons: i32,
    pub origin_country: Vec<String>,
    pub original_language: String,
    pub original_name: String,
    pub overview: String,
    pub popularity: f64,
    pub poster_path: String,
    pub production_companies: Vec<ProductionCompany>,
    pub production_countries: Vec<ProductionCountry>,
    pub seasons: Vec<Season>,
    pub spoken_languages: Vec<Languages>,
    pub status: String,
    pub tagline: String,
    pub r#type: String,
    pub vote_average: f64,
    pub vote_count: i32
}

#[derive(Deserialize, Clone)]
pub struct SeriesCreator {
    pub id: i32,
    pub credit_id: String,
    pub name: String,
    pub gender: i32,
    pub profile_path: String
}

#[derive(Deserialize, Clone)]
pub struct SeriesEpisode {
    pub id: i32,
    pub name: String,
    pub overview: String,
    pub vote_average: i32,
    pub vote_count: i32,
    pub air_date: String,
    pub episode_number: i32,
    pub production_code: String,
    pub runtime: i32,
    pub season_number: i32,
    pub show_id: i32,
    pub still_path: String
}

#[derive(Deserialize, Clone)]
pub struct SeriesNetwork {
    pub id: i32,
    pub logo_path: String,
    pub name: String,
    pub origin_country: String
}

#[derive(Deserialize, Clone)]
pub struct Season {
    pub air_date: String,
    pub episode_count: i32,
    pub id: i32,
    pub name: String,
    pub overview: String,
    pub poster_path: String,
    pub season_number: String,
    pub vote_average: String
}