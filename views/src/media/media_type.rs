use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub enum MediaType {
    #[serde(alias = "movie")]
    Movie,
    #[serde(alias = "series")]
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