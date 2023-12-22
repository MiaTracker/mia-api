use std::env;
use chrono::NaiveDate;
use sea_orm::ActiveValue::Set;
use sea_orm::NotSet;
use serde::Deserialize;
use entities::{media, seasons, series};
use entities::sea_orm_active_enums::MediaType;
use crate::infrastructure::traits::IntoActiveModel;
use crate::tmdb::{Languages, ProductionCompany};
use crate::tmdb::genre::Genre;
use crate::tmdb::production_country::ProductionCountry;

#[derive(Deserialize, Clone)]
pub struct SeriesDetails {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub created_by: Option<Vec<SeriesCreator>>,
    pub episode_run_time: Option<Vec<i32>>,
    pub first_air_date: Option<String>,
    pub genres: Vec<Genre>,
    pub homepage: Option<String>,
    pub id: i32,
    pub in_production: Option<bool>,
    pub languages: Option<Vec<String>>,
    pub last_air_date: Option<String>,
    pub last_episode_to_air: Option<SeriesEpisode>,
    pub name: String,
    pub next_episode_to_air: Option<SeriesEpisode>,
    pub networks: Option<Vec<SeriesNetwork>>,
    pub number_of_episodes: Option<i32>,
    pub number_of_seasons: Option<i32>,
    pub origin_country: Option<Vec<String>>,
    pub original_language: Option<String>,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub popularity: f64,
    pub poster_path: Option<String>,
    pub production_companies: Option<Vec<ProductionCompany>>,
    pub production_countries: Option<Vec<ProductionCountry>>,
    pub seasons: Option<Vec<Season>>,
    pub spoken_languages: Option<Vec<Languages>>,
    pub status: Option<String>,
    pub tagline: Option<String>,
    pub r#type: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>
}

#[derive(Deserialize, Clone)]
pub struct SeriesCreator {
    pub id: i32,
    pub credit_id: Option<String>,
    pub name: String,
    pub gender: Option<i32>,
    pub profile_path: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct SeriesEpisode {
    pub id: i32,
    pub name: String,
    pub overview: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>,
    pub air_date: Option<String>,
    pub episode_number: Option<i32>,
    pub production_code: Option<String>,
    pub runtime: Option<i32>,
    pub season_number: Option<i32>,
    pub show_id: Option<i32>,
    pub still_path: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct SeriesNetwork {
    pub id: i32,
    pub logo_path: Option<String>,
    pub name: String,
    pub origin_country: Option<String>
}

#[derive(Deserialize, Clone)]
pub struct Season {
    pub air_date: Option<String>,
    pub episode_count: Option<i32>,
    pub id: i32,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub season_number: Option<i32>,
    pub vote_average: Option<f32>
}

impl IntoActiveModel<media::ActiveModel> for &SeriesDetails {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: NotSet,
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
            imdb_id: NotSet,
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
            r#type: Set(MediaType::Series),
        }
    }
}

impl IntoActiveModel<series::ActiveModel> for &SeriesDetails {
    fn into_active_model(self) -> series::ActiveModel {
        series::ActiveModel {
            id: NotSet,
            first_air_date:
                if let Some(first_air_date) = self.first_air_date.clone() {
                    if first_air_date.is_empty() { Set(None) }
                    else {
                        let res = NaiveDate::parse_from_str(first_air_date.as_str(), "%Y-%m-%d");
                        match res {
                            Ok(date) => { Set(Some(date)) }
                            Err(_) => { Set(None) }
                        }
                    }
                } else {
                    Set(None)
                },
            number_of_episodes: Set(self.number_of_episodes),
            number_of_seasons: Set(self.number_of_seasons),
            status: Set(self.status.clone().unwrap_or(env::var("UNSET_MEDIA_STATUS").expect("UNSET_MEDIA_STATUS not set!"))),
            r#type: Set(self.r#type.clone()),
        }
    }
}

impl IntoActiveModel<seasons::ActiveModel> for &Season {
    fn into_active_model(self) -> seasons::ActiveModel {
        seasons::ActiveModel {
            id: NotSet,
            series_id: NotSet,
            air_date:
                if let Some(air_date) = self.air_date.clone() {
                    if air_date.is_empty() { Set(None) }
                    else {
                        let res = NaiveDate::parse_from_str(air_date.as_str(), "%Y-%m-%d");
                        match res {
                            Ok(date) => { Set(Some(date)) }
                            Err(_) => { Set(None) }
                        }
                    }
                } else {
                    Set(None)
                },
            episode_count: Set(self.episode_count),
            name: Set(self.name.clone()),
            overview: Set(self.overview.clone()),
            poster_path: Set(self.poster_path.clone()),
            season_number: Set(self.season_number),
            tmdb_vote_average: Set(self.vote_average),
            stars: NotSet,
        }
    }
}