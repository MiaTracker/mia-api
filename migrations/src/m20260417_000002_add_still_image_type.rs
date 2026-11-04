use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::{ConnectOptions, Database, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

const ADD_STILL: &str = "ALTER TYPE image_type ADD VALUE IF NOT EXISTS 'still'";

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // SeaORM runs all pending Postgres migrations in one transaction, and a value that
        // ALTER TYPE ... ADD VALUE adds inside a transaction can't be used before it commits
        // (m20260417_000003 inserts 'still' images). So when `image_type` is already committed,
        // the value is added and committed through a separate autocommit connection.
        //
        // When `image_type` was created earlier in this same run, that connection can't see it.
        // The value is then added in the run's transaction, where it is usable right away: a value
        // of a type created in the same transaction is safe to use.
        let cfg = infrastructure::config();
        let connect_options = ConnectOptions::new(&cfg.db.connection_url)
            .set_schema_search_path(&cfg.db.schema)
            .to_owned();
        let fresh_db = Database::connect(connect_options)
            .await
            .map_err(|e| DbErr::Custom(format!("Failed to open connection for enum migration: {}", e)))?;
        let backend = manager.get_database_backend();

        let committed = fresh_db.query_one(Statement::from_string(
            backend,
            "SELECT to_regtype('image_type') IS NOT NULL AS committed".to_string(),
        )).await?
            .ok_or_else(|| DbErr::Custom("Checking for image_type returned no row".to_string()))?
            .try_get::<bool>("", "committed")?;

        if committed {
            fresh_db.execute(Statement::from_string(backend, ADD_STILL.to_string())).await?;
        } else {
            manager.get_connection().execute(Statement::from_string(backend, ADD_STILL.to_string())).await?;
        }
        fresh_db.close().await?;
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // PostgreSQL does not support removing enum values
        Ok(())
    }
}
