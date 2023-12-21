//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.10

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "people")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub tmdb_id: i32,
    pub imdb_id: Option<String>,
    pub name: String,
    pub also_known_as: Option<Vec<String>>,
    pub biography: Option<String>,
    pub birth_day: Option<Date>,
    pub death_day: Option<Date>,
    pub gender: i32,
    pub homepage: Option<String>,
    pub place_of_birth: Option<String>,
    pub profile_path: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::credits::Entity")]
    Credits,
}

impl Related<super::credits::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Credits.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
