use serde::Serialize;
use crate::languages::Language;
use crate::logs::Log;
use crate::tags::Tag;
use crate::titles::AlternativeTitle;
use crate::genres::Genre;
use crate::sources::Source;

#[derive(Serialize)]
pub struct MovieDetails {
    pub id: i32,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub stars: Option<f32>,
    pub title: String,
    pub alternative_titles: Vec<AlternativeTitle>,
    pub release_date: Option<chrono::NaiveDate>,
    pub runtime: Option<i32>,
    pub status: Option<String>,
    pub overview: Option<String>,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<Language>,
    pub genres: Vec<Genre>,
    pub tags: Vec<Tag>,
    pub sources: Vec<Source>,
    pub logs: Vec<Log>
}