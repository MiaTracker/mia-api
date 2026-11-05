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

impl From<RouteType> for MediaType {
    fn from(val: RouteType) -> Self {
        match val {
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

impl From<MaybeRouteType> for Option<MediaType> {
    fn from(val: MaybeRouteType) -> Self {
        match val {
            MaybeRouteType::All => { None }
            MaybeRouteType::Movies => { Some(MediaType::Movie) }
            MaybeRouteType::Series => { Some(MediaType::Series) }
        }
    }
}