//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.10

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "seasons")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub series_id: i32,
    pub air_date: Option<Date>,
    pub episode_count: Option<i32>,
    pub name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub season_number: Option<i32>,
    #[sea_orm(column_type = "Float", nullable)]
    pub tmdb_vote_average: Option<f32>,
    #[sea_orm(column_type = "Float", nullable)]
    pub stars: Option<f32>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::series::Entity",
        from = "Column::SeriesId",
        to = "super::series::Column::Id",
        on_update = "Cascade",
        on_delete = "Cascade"
    )]
    Series,
}

impl Related<super::series::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Series.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
