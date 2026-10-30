use std::sync::Arc;
use sea_orm::DatabaseConnection;
use services::chat::ExchangeRegistry;
use services::infrastructure::{Clock, ImageStore, Llm, Random, Tmdb};
use services::jobs::Jobs;

#[derive(Clone)]
pub struct AppState {
    /// Shared because `DatabaseConnection` is not `Clone` when sea-orm's `mock` feature is on (test builds).
    pub conn: Arc<DatabaseConnection>,
    pub jwt_secret: String,
    pub tmdb: Arc<dyn Tmdb>,
    pub image_store: Arc<dyn ImageStore>,
    /// Not read by any handler yet: exchanges reach the model through `jobs`, which holds its own handle.
    #[allow(dead_code)]
    pub llm: Arc<dyn Llm>,
    pub registry: Arc<ExchangeRegistry>,
    pub jobs: Arc<dyn Jobs>,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
}
