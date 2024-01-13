use integrations::tmdb;

pub async fn initialize() {
    tmdb::services::initialize::initialize().await;
}