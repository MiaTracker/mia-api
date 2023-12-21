//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.10

use super::sea_orm_active_enums::CreditsType;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "credits")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub media_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub person_id: i32,
    #[sea_orm(primary_key, auto_increment = false)]
    pub r#type: CreditsType,
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
    #[sea_orm(
        belongs_to = "super::people::Entity",
        from = "Column::PersonId",
        to = "super::people::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    People,
}

impl Related<super::media::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Media.def()
    }
}

impl Related<super::people::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::People.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
