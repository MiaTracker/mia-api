use integrations::tmdb::Tmdb;
use crate::infrastructure::constants::set_tmdb_config;

/// Fetches the TMDB configuration and stores it for `tmdb_config()`. Call once at startup.
pub async fn initialize(tmdb: &dyn Tmdb) {
    let configuration = tmdb.configuration().await.expect("Failed to load TMDB configuration");
    set_tmdb_config(&configuration);
}
