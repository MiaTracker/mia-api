use chrono::NaiveDate;
use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use serde::Deserialize;
use entities::{genres, media, movies};
use entities::sea_orm_active_enums::MediaType;
use crate::infrastructure::traits::IntoActiveModel;
use crate::tmdb::genre::Genre;
use crate::tmdb::{Languages, ProductionCompany, ProductionCountry};

#[derive(Deserialize, Clone)]
pub struct MovieDetails {
    pub adult: bool,
    pub backdrop_path: String,
    pub belongs_to_collection: Option<String>,
    pub budget: i32,
    pub genres: Vec<Genre>,
    pub homepage: String,
    pub id: i32,
    pub imdb_id: String,
    pub original_language: String,
    pub original_title: String,
    pub overview: String,
    pub popularity: f32,
    pub poster_path: String,
    pub production_companies: Vec<ProductionCompany>,
    pub production_countries: Vec<ProductionCountry>,
    pub release_date: String,
    pub revenue: i32,
    pub runtime: i32,
    pub spoken_languages: Vec<Languages>,
    pub status: String,
    pub tagline: String,
    pub title: String,
    pub video: bool,
    pub vote_average: f32,
    pub vote_count: i32
}

impl IntoActiveModel<media::ActiveModel> for &MovieDetails {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: NotSet,
            backdrop_path: if self.backdrop_path.is_empty() { Set(None) } else { Set(Some(self.backdrop_path.clone())) },
            homepage: if self.homepage.is_empty() { Set(None) } else { Set(Some(self.homepage.clone())) },
            tmdb_id: Set(Some(self.id)),
            imdb_id: if self.imdb_id.is_empty() { Set(None) } else { Set(Some(self.imdb_id.clone())) },
            overview: if self.overview.is_empty() { Set(None) } else { Set(Some(self.overview.clone())) },
            poster_path: if self.poster_path.is_empty() { Set(None) } else { Set(Some(self.poster_path.clone())) },
            tmdb_vote_average: Set(Some(self.vote_average)),
            original_language: if self.original_language.is_empty() { Set(None) } else { Set(Some(self.original_language.clone()))},
            logged: Set(true),
            date_added: Set(chrono::Utc::now().date_naive()),
            r#type: Set(MediaType::Movie),
        }
    }
}

impl IntoActiveModel<movies::ActiveModel> for &MovieDetails {
    fn into_active_model(self) -> movies::ActiveModel {
        movies::ActiveModel {
            id: NotSet,
            release_date: if self.release_date.is_empty() {
                Set(NaiveDate::default())
            } else {
                let res = NaiveDate::parse_from_str(self.release_date.as_str(), "%Y-%m-%d");
                match res {
                    Ok(date) => { Set(date) }
                    Err(_) => { Set(NaiveDate::default()) }
                }
            },
            runtime: Set(Some(self.runtime)),
            status: Set(self.status.clone()),
        }
    }
}



impl IntoActiveModel<genres::ActiveModel> for &Genre {
    fn into_active_model(self) -> genres::ActiveModel {
        genres::ActiveModel {
            id: NotSet,
            tmdb_id: Set(Some(self.id)),
            name: Set(self.name.clone()),
        }
    }
}