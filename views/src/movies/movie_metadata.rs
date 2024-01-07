use cruet::Inflector;
use sea_orm::{NotSet, Set};
use serde::{Deserialize, Serialize};
use entities::{media, movies};
use entities::sea_orm_active_enums::MediaType::Movie;
use crate::infrastructure::traits::IntoActiveModel;

#[derive(Serialize, Deserialize)]
pub struct MovieMetadata {
    pub id: i32,
    pub backdrop_path: Option<String>,
    pub homepage: Option<String>,
    pub tmdb_id: Option<i32>,
    pub imdb_id: Option<String>,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<String>,
    pub release_date: Option<chrono::NaiveDate>,
    pub runtime: Option<i32>,
    pub status: Option<String>
}

impl IntoActiveModel<media::ActiveModel> for &MovieMetadata {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: Set(self.id),
            backdrop_path: Set(self.backdrop_path.clone()),
            homepage: Set(self.homepage.clone()),
            tmdb_id: Set(self.tmdb_id),
            imdb_id: Set(self.imdb_id.clone()),
            overview: Set(self.overview.clone()),
            poster_path: Set(self.poster_path.clone()),
            tmdb_vote_average: Set(self.tmdb_vote_average),
            original_language: Set(self.original_language.clone()),
            date_added: NotSet,
            r#type: Set(Movie),
            user_id: NotSet,
            stars: NotSet,
        }
    }
}

impl IntoActiveModel<movies::ActiveModel> for &MovieMetadata {
    fn into_active_model(self) -> movies::ActiveModel {
        movies::ActiveModel {
            id: Set(self.id),
            release_date: Set(self.release_date),
            runtime: Set(self.runtime),
            status: Set(self.status.as_ref().map(|s| { s.to_title_case() })),
        }
    }
}