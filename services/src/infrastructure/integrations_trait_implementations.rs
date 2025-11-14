use chrono::NaiveDate;
use sea_orm::{NotSet, Set};

use crate::infrastructure::constants::tmdb_config;
use crate::infrastructure::traits::{IntoActiveModel, IntoImage, IntoView};
use entities::sea_orm_active_enums::MediaType;
use entities::{genres, languages, media, movies, seasons, series};
use infrastructure::config;
use integrations::tmdb::views::{Genre, Languages, MovieDetails, MultiMovieResult, MultiTvResult, Season, SeriesDetails, TmdbImage};
use views::images::{Image, ImageCandidate};
use views::media::ExternalIndex;

impl IntoActiveModel<media::ActiveModel> for &MovieDetails {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: NotSet,
            user_id: NotSet,
            backdrop_image_id: NotSet,
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
            poster_image_id: NotSet,
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
            stars: Set(None),
            bot_controllable: Set(false)
        }
    }
}

impl IntoActiveModel<movies::ActiveModel> for &MovieDetails {
    fn into_active_model(self) -> movies::ActiveModel {
        movies::ActiveModel {
            id: NotSet,
            release_date: if let Some(release_date) = self.release_date.clone()  {
                if release_date.is_empty() { Set(None) }
                else {
                    let res = NaiveDate::parse_from_str(release_date.as_str(), "%Y-%m-%d");
                    match res {
                        Ok(date) => { Set(Some(date)) }
                        Err(_) => { Set(None) }
                    }
                }
            } else {
                Set(None)
            },
            runtime: Set(self.runtime),
            status: Set(self.status.clone())
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


impl IntoActiveModel<media::ActiveModel> for &SeriesDetails {
    fn into_active_model(self) -> media::ActiveModel {
        media::ActiveModel {
            id: NotSet,
            user_id: NotSet,
            backdrop_image_id: NotSet,
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
            poster_image_id: NotSet,
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
            stars: Set(None),
            bot_controllable: Set(false)
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
            status: Set(self.status.clone()),
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

impl IntoActiveModel<languages::ActiveModel> for &Languages {
    fn into_active_model(self) -> languages::ActiveModel {
        languages::ActiveModel {
            iso6391: Set(self.iso_639_1.clone()),
            english_name: Set(self.english_name.clone()),
            name: Set(self.name.clone()),
        }
    }
}

impl IntoView<ExternalIndex> for &MultiMovieResult {
    fn into_view(self) -> ExternalIndex {
        ExternalIndex {
            external_id: self.id,
            r#type: views::media::MediaType::Movie,
            poster: self.poster_path.clone().map(|p|
                Image {
                    path: p,
                    sizes: tmdb_config().images.poster_sizes.iter().map(|s| s.into_view()).collect(),
                }
            ),
            title: if let Some(title) = self.title.clone() {
                title
            } else {
                config().media.unset_title.clone()
            }
        }
    }
}

impl IntoView<ExternalIndex> for &MultiTvResult {
    fn into_view(self) -> ExternalIndex {
        ExternalIndex {
            external_id: self.id,
            r#type: views::media::MediaType::Series,
            poster: self.poster_path.clone().map(|p|
                Image {
                    path: p,
                    sizes: tmdb_config().images.poster_sizes.iter().map(|s| s.into_view()).collect(),
                }
            ),
            title: if let Some(title) = self.name.clone() {
                title
            } else {
                config().media.unset_title.clone()
            },
        }
    }
}

impl IntoView<ExternalIndex> for &MovieDetails {
    fn into_view(self) -> ExternalIndex {
        ExternalIndex {
            external_id: self.id,
            r#type: views::media::MediaType::Movie,
            poster: self.poster_path.clone().map(|p|
                Image {
                    path: p,
                    sizes: tmdb_config().images.poster_sizes.iter().map(|s| s.into_view()).collect(),
                }
            ),
            title: self.title.clone(),
        }
    }
}

impl IntoView<ExternalIndex> for &SeriesDetails {
    fn into_view(self) -> ExternalIndex {
        ExternalIndex {
            external_id: self.id,
            r#type: views::media::MediaType::Series,
            poster: self.poster_path.clone().map(|p|
                Image {
                    path: p,
                    sizes: tmdb_config().images.poster_sizes.iter().map(|s| s.into_view()).collect(),
                }
            ),
            title: self.name.clone(),
        }
    }
}

impl IntoImage for &TmdbImage {
    fn into_image(self, languages: &Vec<languages::Model>, sizes: &Vec<views::configuration::ImageSize>) -> ImageCandidate {
        ImageCandidate {
            language: self.iso_639_1.as_ref().map(|z| languages.iter().find(|y| y.iso6391.as_str() == z).map(|l| l.english_name.clone())).flatten(),
            original_width: self.width,
            original_height: self.height,
            path: self.file_path.clone(),
            current: false,
            sizes: sizes.iter().map(|s| s.into_view()).collect(),
            source: views::images::ImageSource::TMDB,
        }
    }
}