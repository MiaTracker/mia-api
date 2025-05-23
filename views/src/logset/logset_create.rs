use chrono::NaiveDate;
use serde::Deserialize;
use utoipa::ToSchema;
use crate::media::MediaType;
use crate::sources::SourceCreate;

#[derive(Deserialize, ToSchema)]
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
    #[schema(value_type = String, format = Date)]
    pub date: NaiveDate,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>,
    #[serde(default)]
    pub remove_from_watchlist: Option<bool>
}