use serde::Deserialize;
use crate::media::MediaType;

#[derive(Deserialize)]
pub struct MediaCreateParams {
    pub tmdb_id: i32,
    pub r#type: MediaType
}