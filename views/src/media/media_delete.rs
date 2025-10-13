use serde::Deserialize;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
pub struct MovieDeletePathParams {
    #[serde(alias = "movie_id")]
    #[serde(alias = "series_id")]
    pub movie_id: i32
}

#[derive(Deserialize, IntoParams)]
pub struct SeriesDeletePathParams {
    #[serde(alias = "movie_id")]
    #[serde(alias = "series_id")]
    pub series_id: i32
}