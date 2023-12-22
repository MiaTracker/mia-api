use serde::Serialize;
use crate::genres::Genre;
use crate::languages::Language;
use crate::logs::Log;
use crate::sources::Source;
use crate::tags::Tag;
use crate::titles::AlternativeTitle;

#[derive(Serialize)]
pub struct SeriesDetails {
    pub id: i32,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub stars: Option<f32>,
    pub title: String,
    pub alternative_titles: Vec<AlternativeTitle>,
    pub first_air_date: Option<chrono::NaiveDate>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub status: String,
    pub r#type: Option<String>,
    pub overview: Option<String>,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<Language>,
    pub genres: Vec<Genre>,
    pub tags: Vec<Tag>,
    pub sources: Vec<Source>,
    pub logs: Vec<Log>
}