use serde::Deserialize;

#[derive(Deserialize)]
pub struct MediaSourceDelete {
    pub tmdb_id: i32,
    pub source: String
}