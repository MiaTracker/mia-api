use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectOptions, Database, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // ALTER TYPE ADD VALUE cannot be used in the same transaction where the new value is
        // subsequently used. SeaORM wraps all pending Postgres migrations in one transaction,
        // so we commit the enum addition via a separate autocommit connection instead.
        let cfg = infrastructure::config();
        let connect_options = ConnectOptions::new(&cfg.db.connection_url)
            .set_schema_search_path(&cfg.db.schema)
            .to_owned();
        let fresh_db = Database::connect(connect_options)
            .await
            .map_err(|e| DbErr::Custom(format!("Failed to open connection for enum migration: {}", e)))?;
        fresh_db.execute(Statement::from_string(
            manager.get_database_backend(),
            "ALTER TYPE image_type ADD VALUE IF NOT EXISTS 'still'".to_string(),
        )).await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // PostgreSQL does not support removing enum values
        Ok(())
    }
}
