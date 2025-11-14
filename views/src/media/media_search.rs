use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use crate::images::Image;
use crate::media::{MediaIndex, MediaType, PageReq};

#[derive(Deserialize, IntoParams)]
pub struct SearchParams {
    pub committed: bool,
    #[serde(default)]
    pub offset: Option<u64>,
    #[serde(default)]
    pub limit: Option<u64>
}

impl Into<PageReq> for SearchParams {
    fn into(self) -> PageReq {
        PageReq {
            offset: self.offset,
            limit: self.limit,
        }
    }
}

#[derive(Deserialize, ToSchema)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default)]
    pub genres: Option<Vec<String>>,
    #[serde(default)]
    pub only_watched: bool,
    #[serde(default)]
    pub min_stars: Option<f32>,
    #[serde(default)]
    pub sort_by: SortTarget
}

#[derive(Deserialize, ToSchema)]
pub enum SortTarget {
    #[serde(rename = "title")]
    Title,
    #[serde(rename = "stars")]
    Stars,
    #[serde(rename = "times_watched")]
    TimesWatched
}

impl Default for SortTarget {
    fn default() -> Self {
        Self::Title
    }
}

#[derive(Serialize, ToSchema)]
pub struct SearchResults {
    pub indexes: Vec<MediaIndex>,
    pub external: Vec<ExternalIndex>,
    pub query_valid: bool
}

#[derive(Serialize, ToSchema)]
pub struct ExternalIndex {
    pub external_id: i32,
    pub r#type: MediaType,
    pub poster: Option<Image>,
    pub title: String
}