use serde::Deserialize;

#[derive(Deserialize)]
pub struct MediaDeletePathParams {
    #[serde(alias = "movie_id")]
    #[serde(alias = "series_id")]
    pub media_id: i32
}