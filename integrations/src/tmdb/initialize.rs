use crate::constants;
use crate::tmdb::configuration;

pub async fn initialize() {
    let _ = constants::TMDB_CONFIGURATION.set(configuration::details().await.expect("Failed to initialize TMDB_CONFIGURATION"));
}