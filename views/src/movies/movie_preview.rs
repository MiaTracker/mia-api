use serde::Serialize;
use utoipa::ToSchema;
use crate::genres::Genre;
use crate::images::Image;
use crate::languages::Language;

/// A movie as TMDB describes it, for one that is not in the library yet. The fields of
/// `MovieDetails` that need a library row (`id`, `stars`, `tags`, `sources`, `logs`, `locks`,
/// `on_watchlist`) are left out. The images are TMDB images (`tmdb/<size>` slugs).
#[derive(Serialize, ToSchema)]
pub struct MoviePreview {
    pub poster: Option<Image>,
    pub backdrop: Option<Image>,
    pub title: String,
    pub alternative_titles: Vec<String>,
    #[schema(value_type = String, format = Date)]
    pub release_date: Option<chrono::NaiveDate>,
    pub runtime: Option<i32>,
    pub status: Option<String>,
    pub overview: Option<String>,
    pub tmdb_id: i32,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<Language>,
    pub genres: Vec<Genre>
}
