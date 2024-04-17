use serde::Deserialize;
use utoipa::ToSchema;
use crate::sources::SourceCreate;

#[derive(Deserialize, ToSchema)]
pub struct MediaSourceCreate {
    pub tmdb_id: i32,
    pub source: SourceCreate
}