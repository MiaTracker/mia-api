use std::sync::OnceLock;
use crate::tmdb::views::Configuration;

pub const TMDB_URL: &str = "https://api.themoviedb.org/3/";
pub const TMDB_LANG: &str = "en-US";
pub const TMDB_COUNTRY: &str = "US";
pub const TMDB_ISO_LANG: &str = "en";
pub static TMDB_CONFIGURATION: OnceLock<Configuration> = OnceLock::new();