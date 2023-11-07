use serde::Serialize;
use entities::sources::Model;

#[derive(Serialize)]
pub struct Source {
    pub id: i32,
    pub name: String,
    pub url: String,
    pub r#type: SourceType
}

#[derive(Serialize)]
pub enum SourceType {
    #[serde(rename = "torrent")]
    Torrent,
    #[serde(rename = "web")]
    Web,
    #[serde(rename = "jellyfin")]
    Jellyfin
}

impl From<&Model> for Source {
    fn from(value: &Model) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
            url: value.url.clone(),
            r#type: SourceType::from(&value.r#type),
        }
    }
}

impl From<&entities::sea_orm_active_enums::SourceType> for SourceType {
    fn from(value: &entities::sea_orm_active_enums::SourceType) -> Self {
        match value {
            entities::sea_orm_active_enums::SourceType::Torrent => { SourceType::Torrent }
            entities::sea_orm_active_enums::SourceType::Web => { SourceType::Web }
            entities::sea_orm_active_enums::SourceType::Jellyfin => { SourceType::Jellyfin }
        }
    }
}