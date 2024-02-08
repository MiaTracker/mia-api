use serde::Deserialize;
use crate::sources::SourceCreate;

#[derive(Deserialize)]
pub struct MediaSourceCreate {
    pub tmdb_id: i32,
    pub source: SourceCreate
}