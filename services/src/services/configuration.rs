use crate::infrastructure::constants::tmdb_config;
use crate::infrastructure::SrvErr;
use views::configuration::ImagesConfiguration;

pub async fn images() -> Result<ImagesConfiguration, SrvErr> {
    Ok(tmdb_config().images.clone())
}