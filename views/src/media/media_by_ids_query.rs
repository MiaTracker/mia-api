use serde::Deserialize;
use utoipa::ToSchema;
use crate::media::{SearchQuery, SortTarget};

#[derive(Deserialize, ToSchema)]
pub struct MediaByIdsQuery {
    pub query: String,
    pub ids: Vec<i32>,
}

impl From<MediaByIdsQuery> for SearchQuery {
    fn from(v: MediaByIdsQuery) -> Self {
        SearchQuery {
            query: v.query,
            genres: None,
            only_watched: false,
            min_stars: None,
            sort_by: SortTarget::default(),
        }
    }
}
