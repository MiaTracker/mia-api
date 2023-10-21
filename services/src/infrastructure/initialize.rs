use integrations::tmdb;

pub async fn initialize() {
    tmdb::initialize::initialize().await;
}