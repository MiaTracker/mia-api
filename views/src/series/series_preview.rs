use serde::Serialize;
use utoipa::ToSchema;
use crate::genres::Genre;
use crate::images::Image;
use crate::languages::Language;

/// A series as TMDB describes it, for one that is not in the library yet. The fields of
/// `SeriesDetails` that need a library row (`id`, `stars`, `tags`, `sources`, `logs`, `locks`,
/// `on_watchlist`) are left out. The images are TMDB images (`tmdb/<size>` slugs), and the season
/// and episode counts are TMDB's.
#[derive(Serialize, ToSchema)]
pub struct SeriesPreview {
    pub poster: Option<Image>,
    pub backdrop: Option<Image>,
    pub title: String,
    pub alternative_titles: Vec<String>,
    #[schema(value_type = String, format = Date)]
    pub first_air_date: Option<chrono::NaiveDate>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub status: Option<String>,
    pub r#type: Option<String>,
    pub overview: Option<String>,
    pub tmdb_id: i32,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<Language>,
    pub genres: Vec<Genre>
}
