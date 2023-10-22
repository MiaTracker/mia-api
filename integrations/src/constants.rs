use std::sync::OnceLock;
use views::tmdb::Configuration;

pub const TMDB_URL: &str = "https://api.themoviedb.org/3/";
pub const TMDB_LANG: &str = "en-US";
pub const TMDB_COUNTRY: &str = "US";
pub static TMDB_CONFIGURATION: OnceLock<Configuration> = OnceLock::new();