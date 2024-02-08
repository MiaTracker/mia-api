//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.10

use super::sea_orm_active_enums::MediaType;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "media")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub backdrop_path: Option<String>,
    pub homepage: Option<String>,
    pub tmdb_id: Option<i32>,
    pub imdb_id: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    #[sea_orm(column_type = "Float", nullable)]
    pub tmdb_vote_average: Option<f32>,
    pub original_language: Option<String>,
    pub date_added: Date,
    pub r#type: MediaType,
    pub user_id: i32,
    #[sea_orm(column_type = "Float", nullable)]
    pub stars: Option<f32>,
    pub bot_controllable: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::credits::Entity")]
    Credits,
    #[sea_orm(has_many = "super::images::Entity")]
    Images,
    #[sea_orm(
        belongs_to = "super::languages::Entity",
        from = "Column::OriginalLanguage",
        to = "super::languages::Column::Iso6391",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Languages,
    #[sea_orm(has_many = "super::logs::Entity")]
    Logs,
    #[sea_orm(has_many = "super::media_genres::Entity")]
    MediaGenres,
    #[sea_orm(has_many = "super::media_tags::Entity")]
    MediaTags,
    #[sea_orm(has_many = "super::movies::Entity")]
    Movies,
    #[sea_orm(has_many = "super::series::Entity")]
    Series,
    #[sea_orm(has_many = "super::sources::Entity")]
    Sources,
    #[sea_orm(has_many = "super::titles::Entity")]
    Titles,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::UserId",
        to = "super::users::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Users,
    #[sea_orm(has_many = "super::watchlist::Entity")]
    Watchlist,
}

impl Related<super::credits::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Credits.def()
    }
}

impl Related<super::images::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Images.def()
    }
}

impl Related<super::languages::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Languages.def()
    }
}

impl Related<super::logs::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Logs.def()
    }
}

impl Related<super::media_genres::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MediaGenres.def()
    }
}

impl Related<super::media_tags::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::MediaTags.def()
    }
}

impl Related<super::movies::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Movies.def()
    }
}

impl Related<super::series::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Series.def()
    }
}

impl Related<super::sources::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Sources.def()
    }
}

impl Related<super::titles::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Titles.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Users.def()
    }
}

impl Related<super::watchlist::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Watchlist.def()
    }
}

impl Related<super::genres::Entity> for Entity {
    fn to() -> RelationDef {
        super::media_genres::Relation::Genres.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::media_genres::Relation::Media.def().rev())
    }
}

impl Related<super::tags::Entity> for Entity {
    fn to() -> RelationDef {
        super::media_tags::Relation::Tags.def()
    }
    fn via() -> Option<RelationDef> {
        Some(super::media_tags::Relation::Media.def().rev())
    }
}

impl ActiveModelBehavior for ActiveModel {}
