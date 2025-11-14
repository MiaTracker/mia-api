use std::cmp::Ordering;
use std::path::Path;
use cruet::Inflector;
use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use entities::{image_sizes, images, logs, media, movies, series, users};
use entities::sea_orm_active_enums::MediaType::{Movie, Series};
use views::configuration;
use views::configuration::ImageSizeDimension;
use views::images::{ImageCandidate, ImageSize};
use views::logs::LogCreate;
use views::movies::MovieMetadata;
use views::series::SeriesMetadata;
use views::users::UserRegistration;
use crate::infrastructure::constants::TMDB_IMAGE_PREFIX;
use crate::infrastructure::SrvErr;
use crate::infrastructure::traits::{IntoActiveModel, IntoView, SortCompare};

impl IntoActiveModel<users::ActiveModel> for &UserRegistration {
    fn into_active_model(self) -> users::ActiveModel {
        users::ActiveModel {
            id: NotSet,
            uuid: Set(uuid::Uuid::new_v4()),
            email: Set(self.email.clone()),
            username: Set(self.username.clone()),
            password_hash: NotSet,
            admin: Set(false),
        }
    }
}

impl IntoActiveModel<logs::ActiveModel> for &LogCreate {
    fn into_active_model(self) -> logs::ActiveModel {
        logs::ActiveModel {
            id: NotSet,
            media_id: NotSet,
            date: Set(self.date),
            stars: Set(self.stars),
            completed: Set(self.completed),
            comment: Set(self.comment.clone()),
            source_id: NotSet,
        }
    }
}

impl IntoActiveModel<media::ActiveModel> for &MovieMetadata {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: sea_orm::Set(self.id),
            backdrop_image_id: NotSet,
            homepage: sea_orm::Set(self.homepage.clone()),
            tmdb_id: NotSet,
            imdb_id: sea_orm::Set(self.imdb_id.clone()),
            overview: sea_orm::Set(self.overview.clone()),
            poster_image_id: NotSet,
            tmdb_vote_average: NotSet,
            original_language: sea_orm::Set(self.original_language.clone()),
            date_added: NotSet,
            r#type: sea_orm::Set(Movie),
            user_id: NotSet,
            stars: NotSet,
            bot_controllable: sea_orm::Set(false)
        }
    }
}

impl IntoActiveModel<movies::ActiveModel> for &MovieMetadata {
    fn into_active_model(self) -> movies::ActiveModel {
        movies::ActiveModel {
            id: sea_orm::Set(self.id),
            release_date: sea_orm::Set(self.release_date),
            runtime: sea_orm::Set(self.runtime),
            status: sea_orm::Set(self.status.as_ref().map(|s| { s.to_title_case() })),
        }
    }
}

impl IntoActiveModel<media::ActiveModel> for &SeriesMetadata {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: sea_orm::Set(self.id),
            backdrop_image_id: NotSet,
            homepage: sea_orm::Set(self.homepage.clone()),
            tmdb_id: NotSet,
            imdb_id: sea_orm::Set(self.imdb_id.clone()),
            overview: sea_orm::Set(self.overview.clone()),
            poster_image_id: NotSet,
            tmdb_vote_average: NotSet,
            original_language: sea_orm::Set(self.original_language.clone()),
            date_added: NotSet,
            r#type: sea_orm::Set(Series),
            user_id: NotSet,
            stars: NotSet,
            bot_controllable: sea_orm::Set(false)
        }
    }
}

impl IntoActiveModel<series::ActiveModel> for &SeriesMetadata {
    fn into_active_model(self) -> series::ActiveModel {
        series::ActiveModel {
            id: sea_orm::Set(self.id),
            first_air_date: sea_orm::Set(self.first_air_date),
            number_of_episodes: sea_orm::Set(self.number_of_episodes),
            number_of_seasons: sea_orm::Set(self.number_of_seasons),
            status: sea_orm::Set(self.status.clone().map(|s| { s.to_title_case() })),
            r#type: sea_orm::Set(self.r#type.clone().map(|t| { t.to_title_case() })),
        }
    }
}

impl SortCompare for ImageCandidate {
    fn sort_compare(&self, other: &Self) -> Ordering {
        if self.current != other.current {
            if self.current { Ordering::Greater }
            else { Ordering::Less }
        } else {
            (self.original_width * self.original_height).cmp(&(other.original_width * other.original_height))
        }
    }
}

impl IntoView<ImageSize> for &configuration::ImageSize {
    fn into_view(self) -> ImageSize {
        ImageSize {
            slug: format!("{}/{}", TMDB_IMAGE_PREFIX, self.slug),
            width: match self.dimension {
                ImageSizeDimension::Width => { self.size.unwrap_or(i32::MAX) }
                ImageSizeDimension::Height => { i32::MAX }
            },
            height: match self.dimension {
                ImageSizeDimension::Width => { i32::MAX }
                ImageSizeDimension::Height => { self.size.unwrap_or(i32::MAX) }
            },
        }
    }
}

impl IntoView<Result<ImageCandidate, SrvErr>> for &(images::Model, Vec<image_sizes::Model>) {
    fn into_view(self) -> Result<ImageCandidate, SrvErr> {
        let original_size = self.1.iter().max_by(|x, y| x.width.cmp(&y.width))
            .ok_or(SrvErr::Internal("Image did not have a size".to_string()))?;
        Ok(ImageCandidate {
            language: None,
            original_width: original_size.width,
            original_height: original_size.height,
            path: Path::new(&self.0.path).with_extension(self.0.file_type.to_extension()).to_str().unwrap().to_string(),
            sizes: self.1.iter().map(|s| s.into()).collect(),
            current: false,
            source: views::images::ImageSource::Internal,
        })
    }
}