use crate::constants;
use crate::tmdb::services::configuration;

pub async fn initialize() {
    let config = configuration::details().await.expect("Failed to initialize TMDB_CONFIGURATION");
    constants::TMDB_CONFIGURATION.set(config).expect("Failed to set TMDB_CONFIGURATION");
}