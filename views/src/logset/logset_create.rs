use chrono::NaiveDate;
use serde::Deserialize;
use crate::media::MediaType;
use crate::sources::SourceCreate;

#[derive(Deserialize)]
pub struct LogsetCreate {
    #[serde(default)]
    pub media_id: Option<i32>,
    #[serde(default)]
    pub external_id: Option<i32>,
    pub media_type: MediaType,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub new_source: Option<SourceCreate>,
    pub date: NaiveDate,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>,
    #[serde(default)]
    pub remove_from_watchlist: Option<bool>
}