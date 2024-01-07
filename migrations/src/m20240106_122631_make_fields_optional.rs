use sea_orm_migration::prelude::*;
use crate::sea_orm::{DatabaseBackend, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE movies ALTER COLUMN release_date DROP NOT NULL;")).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE movies ALTER COLUMN status DROP NOT NULL;")).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE series ALTER COLUMN status DROP NOT NULL;")).await.map(|_| { () })
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE series ALTER COLUMN status SET NOT NULL;")).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE movies ALTER COLUMN status SET NOT NULL;")).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE movies ALTER COLUMN release_date SET NOT NULL;")).await.map(|_| { () })
    }
}