use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Copy, Clone)]
pub enum MediaType {
    #[serde(rename = "movie")]
    Movie,
    #[serde(rename = "series")]
    Series
}