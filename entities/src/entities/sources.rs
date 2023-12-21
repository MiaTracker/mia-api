//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.10

use super::sea_orm_active_enums::SourceType;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "sources")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub media_id: i32,
    pub name: String,
    pub url: String,
    pub r#type: SourceType,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::media::Entity",
        from = "Column::MediaId",
        to = "super::media::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Media,
}

impl Related<super::media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Media.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
