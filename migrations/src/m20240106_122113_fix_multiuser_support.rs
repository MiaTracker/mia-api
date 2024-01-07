use sea_orm_migration::prelude::*;
use crate::sea_orm::{DatabaseBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE media DROP CONSTRAINT tmdbid_unique_constr;")).await?;
        manager.create_index(
            Index::create().name("tmdbid_unique_constr").table(Media::Table).col(Media::TMDBId).col(Media::Type).col(Media::UserId).unique().to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_index(
            Index::create().name("tmdbid_unique_constr").col(Media::TMDBId).col(Media::Type).unique().to_owned()
        ).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE media DROP CONSTRAINT tmdbid_unique_constr;")).await.map(|_| { () })
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    TMDBId,
    UserId,
    Type
}