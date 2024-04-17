use serde::Deserialize;
use utoipa::ToSchema;
use crate::media::MediaType;

#[derive(Deserialize, ToSchema)]
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

#[derive(Deserialize)]
pub enum MaybeRouteType {
    #[serde(rename = "media")]
    All,
    #[serde(rename = "movies")]
    Movies,
    #[serde(rename = "series")]
    Series
}

impl Into<Option<MediaType>> for MaybeRouteType {
    fn into(self) -> Option<MediaType> {
        match self {
            MaybeRouteType::All => { None }
            MaybeRouteType::Movies => { Some(MediaType::Movie) }
            MaybeRouteType::Series => { Some(MediaType::Series) }
        }
    }
}