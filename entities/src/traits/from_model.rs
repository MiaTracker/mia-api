use views::genres::Genre;
use views::languages::Language;
use views::logs::Log;
use views::media::MediaType;
use views::sources::{Source, SourceType};
use views::tags::Tag;
use views::titles::AlternativeTitle;
use views::users::CurrentUser;
use crate::{entities, genres, languages, logs, sources, tags, titles, users};

impl From<users::Model> for CurrentUser {
    fn from(value: users::Model) -> Self {
        Self {
            id: value.id,
            uuid: value.uuid,
            email: value.email,
            username: value.username,
            admin: value.admin
        }
    }
}

impl From<&tags::Model> for Tag {
    fn from(value: &tags::Model) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
        }
    }
}

impl From<&genres::Model> for Genre {
    fn from(value: &genres::Model) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
        }
    }
}

impl From<&logs::Model> for Log {
    fn from(value: &logs::Model) -> Self {
        Self {
            id: value.id,
            date: value.date,
            rating: value.rating,
            completed: value.completed,
            comment: value.comment.clone(),
        }
    }
}

impl From<languages::Model> for Language {
    fn from(value: languages::Model) -> Self {
        Self {
            iso_639_1: value.iso6391,
            english_name: value.english_name,
            name: value.name,
        }
    }
}

impl From<&titles::Model> for AlternativeTitle {
    fn from(value: &titles::Model) -> Self {
        Self {
            id: value.id,
            title: value.title.clone(),
        }
    }
}

impl From<&sources::Model> for Source {
    fn from(value: &sources::Model) -> Self {
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

impl From<entities::sea_orm_active_enums::MediaType> for MediaType {
    fn from(value: entities::sea_orm_active_enums::MediaType) -> Self {
        match value {
            entities::sea_orm_active_enums::MediaType::Movie => { MediaType::Movie }
            entities::sea_orm_active_enums::MediaType::Series => { MediaType::Series }
        }
    }
}

impl Into<entities::sea_orm_active_enums::MediaType> for MediaType {
    fn into(self) -> entities::sea_orm_active_enums::MediaType {
        match self {
            MediaType::Movie => { entities::sea_orm_active_enums::MediaType::Movie }
            MediaType::Series => { entities::sea_orm_active_enums::MediaType::Series }
        }
    }
}