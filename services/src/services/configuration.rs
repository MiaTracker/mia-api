use integrations::{constants, tmdb};
use crate::infrastructure::SrvErr;

pub async fn images() -> Result<tmdb::views::ImagesConfiguration, SrvErr> {
    let config = constants::TMDB_CONFIGURATION.get().expect("TMDB configuration not initialized!").images.clone();
    Ok(config)
}