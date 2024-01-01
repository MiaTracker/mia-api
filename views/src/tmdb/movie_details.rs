use std::env;
use chrono::NaiveDate;
use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use serde::Deserialize;
use entities::{genres, media, movies};
use entities::sea_orm_active_enums::MediaType;
use crate::infrastructure::traits::IntoActiveModel;
use crate::tmdb::genre::Genre;
use crate::tmdb::{Collection, Languages, ProductionCompany, ProductionCountry};

#[derive(Deserialize, Clone)]
pub struct MovieDetails {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub belongs_to_collection: Option<Collection>,
    pub budget: Option<i32>,
    pub genres: Vec<Genre>,
    pub homepage: Option<String>,
    pub id: i32,
    pub imdb_id: Option<String>,
    pub original_language: Option<String>,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub popularity: f32,
    pub poster_path: Option<String>,
    pub production_companies: Vec<ProductionCompany>,
    pub production_countries: Vec<ProductionCountry>,
    pub release_date: Option<String>,
    pub revenue: Option<i32>,
    pub runtime: Option<i32>,
    pub spoken_languages: Vec<Languages>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub title: String,
    pub video: bool,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>
}

impl IntoActiveModel<media::ActiveModel> for &MovieDetails {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: NotSet,
            user_id: NotSet,
            backdrop_path:
                if let Some(path) = self.backdrop_path.clone() {
                    if path.is_empty() { Set(None) }
                    else { Set(Some(path)) }
                } else { Set(None) },
            homepage:
                if let Some(homepage) = self.homepage.clone() {
                    if homepage.is_empty() { Set(None) }
                    else { Set(Some(homepage)) }
                } else { Set(None) },
            tmdb_id: Set(Some(self.id)),
            imdb_id:
                if let Some(imdb_id) = self.imdb_id.clone() {
                    if imdb_id.is_empty() { Set(None) }
                    else { Set(Some(imdb_id)) }
                } else { Set(None) },
            overview:
                if let Some(overview) = self.overview.clone() {
                    if overview.is_empty() { Set(None) }
                    else { Set(Some(overview)) }
                } else { Set(None) },
            poster_path:
                if let Some(poster_path) = self.poster_path.clone() {
                    if poster_path.is_empty() { Set(None) }
                    else { Set(Some(poster_path)) }
                } else { Set(None) },
            tmdb_vote_average:
                if let Some(vote_average) = self.vote_average {
                    Set(Some(vote_average))
                } else { Set(None) },
            original_language:
                if let Some(original_language) = self.original_language.clone() {
                    if original_language.is_empty() { Set(None) }
                    else { Set(Some(original_language)) }
                } else { Set(None) },
            date_added: Set(chrono::Utc::now().date_naive()),
            r#type: Set(MediaType::Movie),
            stars: Set(None)
        }
    }
}

impl IntoActiveModel<movies::ActiveModel> for &MovieDetails {
    fn into_active_model(self) -> movies::ActiveModel {
        movies::ActiveModel {
            id: NotSet,
            release_date: if let Some(release_date) = self.release_date.clone()  {
                if release_date.is_empty() { Set(NaiveDate::default()) }
                else {
                    let res = NaiveDate::parse_from_str(release_date.as_str(), "%Y-%m-%d");
                    match res {
                        Ok(date) => { Set(date) }
                        Err(_) => { Set(NaiveDate::default()) }
                    }
                }
            } else {
                Set(NaiveDate::default())
            },
            runtime: Set(self.runtime),
            status: Set(self.status.clone().unwrap_or(env::var("UNSET_MEDIA_STATUS").expect("UNSET_MEDIA_STATUS not set!"))),
        }
    }
}



impl IntoActiveModel<genres::ActiveModel> for &Genre {
    fn into_active_model(self) -> genres::ActiveModel {
        genres::ActiveModel {
            id: NotSet,
            tmdb_id: Set(Some(self.id)),
            name: Set(self.name.clone()),
            r#type: NotSet,
        }
    }
}