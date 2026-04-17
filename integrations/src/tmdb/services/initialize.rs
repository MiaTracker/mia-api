use crate::constants;
use crate::tmdb::services::configuration;

pub async fn initialize() {
    if constants::TMDB_CONFIGURATION.get().is_some() {
        return;
    }
    let config = configuration::details().await.expect("Failed to initialize TMDB_CONFIGURATION");
    constants::TMDB_CONFIGURATION.set(config).ok();
}