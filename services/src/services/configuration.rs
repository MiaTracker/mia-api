use crate::infrastructure::constants::tmdb_config;
use crate::infrastructure::SrvErr;
use views::configuration::ImagesConfiguration;

pub async fn images() -> Result<ImagesConfiguration, SrvErr> {
    Ok(tmdb_config().images.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use views::configuration::{ImageSize, ImageSizeDimension};
    use crate::test_support;

    fn slugs(sizes: &[ImageSize]) -> Vec<String> {
        sizes.iter().map(|s| s.slug.clone()).collect()
    }

    #[tokio::test]
    async fn images_returns_tmdb_image_configuration() {
        test_support::init();
        let expected = test_support::config::test_tmdb_configuration().images;

        let images = images().await.expect("images should succeed");

        assert_eq!(images.base_url, expected.base_url);
        assert_eq!(images.secure_base_url, expected.secure_base_url);
        assert_eq!(slugs(&images.backdrop_sizes), expected.backdrop_sizes);
        assert_eq!(slugs(&images.logo_sizes), expected.logo_sizes);
        assert_eq!(slugs(&images.poster_sizes), expected.poster_sizes);
        assert_eq!(slugs(&images.profile_sizes), expected.profile_sizes);
        assert_eq!(slugs(&images.still_sizes), expected.still_sizes);
        // Slugs are parsed into a dimension and a size; "original" has no size.
        let w92 = &images.poster_sizes[0];
        assert_eq!(w92.size, Some(92));
        assert!(matches!(w92.dimension, ImageSizeDimension::Width));
        let h632 = &images.profile_sizes[2];
        assert_eq!(h632.size, Some(632));
        assert!(matches!(h632.dimension, ImageSizeDimension::Height));
        assert_eq!(images.poster_sizes.last().map(|s| s.size), Some(None));
    }
}