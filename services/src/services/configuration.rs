use fancy_regex::Regex;
use once_cell::sync::Lazy;
use integrations::constants;
use views::configuration::{ImageSize, ImageSizeDimension, ImagesConfiguration};
use crate::infrastructure::SrvErr;

static SIZE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^(w|h)(\\d+)$").expect("Failed to create regex")
});

pub async fn images() -> Result<ImagesConfiguration, SrvErr> {
    let config = constants::TMDB_CONFIGURATION.get().expect("TMDB configuration not initialized!").images.clone();
    Ok(ImagesConfiguration {
        base_url: config.base_url,
        secure_base_url: config.secure_base_url,
        backdrop_sizes: config.backdrop_sizes.into_iter().map(parse_size).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        logo_sizes: config.logo_sizes.into_iter().map(parse_size).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        poster_sizes: config.poster_sizes.into_iter().map(parse_size).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        profile_sizes: config.profile_sizes.into_iter().map(parse_size).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
        still_sizes: config.still_sizes.into_iter().map(parse_size).collect::<Result<Vec<ImageSize>, SrvErr>>()?,
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

        let captures = SIZE_REGEX.captures(slug.as_str())
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