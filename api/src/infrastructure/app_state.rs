use std::sync::Arc;
use sea_orm::DatabaseConnection;
use services::infrastructure::{Clock, ImageStore, Random, Tmdb};
use services::jobs::Jobs;

#[derive(Clone)]
pub struct AppState {
    /// Shared because `DatabaseConnection` is not `Clone` when sea-orm's `mock` feature is on (test builds).
    pub conn: Arc<DatabaseConnection>,
    pub jwt_secret: String,
    pub tmdb: Arc<dyn Tmdb>,
    pub image_store: Arc<dyn ImageStore>,
    pub jobs: Arc<dyn Jobs>,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn Random>,
}
