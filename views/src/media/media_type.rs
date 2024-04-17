use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq, ToSchema)]
pub enum MediaType {
    #[serde(rename = "movie")]
    Movie,
    #[serde(rename = "series")]
    Series
}