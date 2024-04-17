use serde::Deserialize;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
pub struct MediaCreateParams {
    pub tmdb_id: i32
}