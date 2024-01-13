use serde::Deserialize;
use crate::media::MediaType;

#[derive(Deserialize)]
pub enum RouteType {
    #[serde(rename = "movies")]
    Movies,
    #[serde(rename = "series")]
    Series
}

impl Into<MediaType> for RouteType {
    fn into(self) -> MediaType {
        match self {
            RouteType::Movies => { MediaType::Movie }
            RouteType::Series => { MediaType::Series }
        }
    }
}