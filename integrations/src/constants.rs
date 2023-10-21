use std::cell::OnceCell;
use views::tmdb::Configuration;

pub const TMDB_URL: &str = "https://api.themoviedb.org/3/";
pub const TMDB_LANG: &str = "en-US";
pub const TMDB_CONFIGURATION: OnceCell<Configuration> = OnceCell::new();