use integrations::constants;
use crate::infrastructure::SrvErr;

pub async fn images() -> Result<views::tmdb::ImagesConfiguration, SrvErr> {
    let config = constants::TMDB_CONFIGURATION.get().expect("TMDB configuration not initialized!").images.clone();
    Ok(config)
}