use inflector::Inflector;
use sea_orm::{NotSet, Set};
use serde::{Deserialize, Serialize};
use entities::{media, series};
use entities::sea_orm_active_enums::MediaType::Series;
use crate::infrastructure::traits::IntoActiveModel;

#[derive(Serialize, Deserialize)]
pub struct SeriesMetadata {
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
    pub first_air_date: Option<chrono::NaiveDate>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub status: Option<String>,
    pub r#type: Option<String>,
}

impl IntoActiveModel<media::ActiveModel> for &SeriesMetadata {
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
            r#type: Set(Series),
            user_id: NotSet,
            stars: NotSet,
        }
    }
}

impl IntoActiveModel<series::ActiveModel> for &SeriesMetadata {
    fn into_active_model(self) -> series::ActiveModel {
        series::ActiveModel {
            id: Set(self.id),
            first_air_date: Set(self.first_air_date),
            number_of_episodes: Set(self.number_of_episodes),
            number_of_seasons: Set(self.number_of_seasons),
            status: Set(self.status.clone().map(|s| { s.to_title_case() })),
            r#type: Set(self.r#type.clone().map(|t| { t.to_title_case() })),
        }
    }
}