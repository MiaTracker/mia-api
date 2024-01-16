use integrations::constants;
use views::configuration::ImagesConfiguration;
use crate::infrastructure::SrvErr;

pub async fn images() -> Result<ImagesConfiguration, SrvErr> {
    let config = constants::TMDB_CONFIGURATION.get().expect("TMDB configuration not initialized!").images.clone();
    Ok(ImagesConfiguration {
        base_url: config.base_url,
        secure_base_url: config.secure_base_url,
        backdrop_sizes: config.backdrop_sizes,
        logo_sizes: config.logo_sizes,
        poster_sizes: config.poster_sizes,
        profile_sizes: config.profile_sizes,
        still_sizes: config.still_sizes,
    })
}