use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Copy, Clone, Eq, PartialEq)]
pub enum MediaType {
    #[serde(rename = "movie")]
    Movie,
    #[serde(rename = "series")]
    Series
}