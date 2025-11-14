use integrations::tmdb;
use crate::infrastructure::constants::set_tmdb_config;

pub async fn initialize() {
    tmdb::services::initialize::initialize().await;
    set_tmdb_config(integrations::constants::TMDB_CONFIGURATION.get().expect("Failed to load TMDB configuration"));
}