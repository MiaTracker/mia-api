use std::sync::{Arc, OnceLock};
use fancy_regex::Regex;
use once_cell::sync::Lazy;
use infrastructure::fail;
use integrations::tmdb;
use views::configuration::{ImageSize, ImageSizeDimension, ImagesConfiguration};
use crate::infrastructure::SrvErr;

pub const TMDB_3166_1: &str = "US";
pub const TMDB_IMAGE_PREFIX: &str = "tmdb";


static TMDB_SIZE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^(w|h)(\\d+)$").expect("Failed to create regex")
});

static TMDB_CONFIGURATION: OnceLock<Arc<TmdbConfiguration>> = OnceLock::new();

pub struct TmdbConfiguration {
    pub images: ImagesConfiguration
}

pub fn tmdb_config() -> Arc<TmdbConfiguration> {
    match TMDB_CONFIGURATION.get() {
        Some(config) => { config.clone() }
        None => {
            fail!("TMDB config was accessed before being initialized! This is an internal developer error.");
        }
    }
}

pub fn set_tmdb_config(config: &tmdb::views::Configuration) {
    let images_config = match build_tmdb_images_config(&config.images) {
        Ok(i) => i,
        Err(err) => {
            fail!("Error parsing tmdb images config: {}", err);
        }
    };

    let tmdb_config = TmdbConfiguration {
        images: images_config,
    };

    match TMDB_CONFIGURATION.set(Arc::new(tmdb_config)) {
        Ok(_) => { }
        Err(_) => {
            fail!("Tried to set tmdb config more than once! This is an internal developer error.");
        }
    }
}

fn build_tmdb_images_config(config: &tmdb::views::ImagesConfiguration) -> Result<ImagesConfiguration, SrvErr> {
    Ok(ImagesConfiguration {
        base_url: config.base_url.clone(),
        secure_base_url: config.secure_base_url.clone(),
        backdrop_sizes: config.backdrop_sizes.iter().map(|s| parse_size(s.clone())).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        logo_sizes: config.logo_sizes.iter().map(|s| parse_size(s.clone())).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        poster_sizes: config.poster_sizes.iter().map(|s| parse_size(s.clone())).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        profile_sizes: config.profile_sizes.iter().map(|s| parse_size(s.clone())).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        still_sizes: config.still_sizes.iter().map(|s| parse_size(s.clone())).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
    })
}

fn parse_size(slug: String) -> Result<ImageSize, SrvErr> {
    fn parse_err() -> SrvErr {
        SrvErr::Integration("Failed to parse image size".into())
    }

    if slug == "original" {
        Ok(ImageSize {
            size: None,
            dimension: ImageSizeDimension::Width,
            slug
        })
    }
    else {

        let captures = TMDB_SIZE_REGEX.captures(slug.as_str())
            .map_err(|_| parse_err())?
            .ok_or_else(parse_err)?;

        let dimension = match captures.get(1).ok_or_else(parse_err)?.as_str() {
            "w" => ImageSizeDimension::Width,
            "h" => ImageSizeDimension::Height,
            _ => return Err(parse_err()),
        };
        let size = captures.get(2)
            .ok_or_else(parse_err)?.as_str()
            .parse::<i32>().map_err(|_| parse_err())?;

        Ok(
            ImageSize {
                size: Some(size),
                dimension,
                slug
            }
        )
    }
}