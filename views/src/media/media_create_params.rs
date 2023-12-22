use serde::Deserialize;

#[derive(Deserialize)]
pub struct MediaCreateParams {
    pub tmdb_id: i32
}