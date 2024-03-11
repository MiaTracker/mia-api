use serde::Deserialize;
use crate::tmdb::views::{Collection, Genre, Languages, ProductionCompany, ProductionCountry};

#[derive(Deserialize, Clone)]
pub struct MovieDetails {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub belongs_to_collection: Option<Collection>,
    pub budget: Option<u32>,
    pub genres: Vec<Genre>,
    pub homepage: Option<String>,
    pub id: i32,
    pub imdb_id: Option<String>,
    pub original_language: Option<String>,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub popularity: f32,
    pub poster_path: Option<String>,
    pub production_companies: Vec<ProductionCompany>,
    pub production_countries: Vec<ProductionCountry>,
    pub release_date: Option<String>,
    pub revenue: Option<u32>,
    pub runtime: Option<i32>,
    pub spoken_languages: Vec<Languages>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub title: String,
    pub video: bool,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>
}