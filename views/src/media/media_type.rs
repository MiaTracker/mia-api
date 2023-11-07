use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub enum MediaType {
    #[serde(rename = "movie")]
    Movie,
    #[serde(rename = "series")]
    Series
}

impl From<entities::sea_orm_active_enums::MediaType> for MediaType {
    fn from(value: entities::sea_orm_active_enums::MediaType) -> Self {
        match value {
            entities::sea_orm_active_enums::MediaType::Movie => { MediaType::Movie }
            entities::sea_orm_active_enums::MediaType::Series => { MediaType::Series }
        }
    }
}