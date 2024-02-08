use cruet::Inflector;
use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use entities::{logs, media, movies, series, users};
use entities::sea_orm_active_enums::MediaType::{Movie, Series};
use views::logs::LogCreate;
use views::movies::MovieMetadata;
use views::series::SeriesMetadata;
use views::users::UserRegistration;
use crate::infrastructure::traits::IntoActiveModel;

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
            backdrop_path: sea_orm::Set(self.backdrop_path.clone()),
            homepage: sea_orm::Set(self.homepage.clone()),
            tmdb_id: sea_orm::Set(self.tmdb_id),
            imdb_id: sea_orm::Set(self.imdb_id.clone()),
            overview: sea_orm::Set(self.overview.clone()),
            poster_path: sea_orm::Set(self.poster_path.clone()),
            tmdb_vote_average: sea_orm::Set(self.tmdb_vote_average),
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
            backdrop_path: sea_orm::Set(self.backdrop_path.clone()),
            homepage: sea_orm::Set(self.homepage.clone()),
            tmdb_id: sea_orm::Set(self.tmdb_id),
            imdb_id: sea_orm::Set(self.imdb_id.clone()),
            overview: sea_orm::Set(self.overview.clone()),
            poster_path: sea_orm::Set(self.poster_path.clone()),
            tmdb_vote_average: sea_orm::Set(self.tmdb_vote_average),
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