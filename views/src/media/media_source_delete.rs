use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct MediaSourceDelete {
    pub tmdb_id: i32,
    pub source: String
}