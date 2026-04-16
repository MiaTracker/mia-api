use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(
            Table::create()
                .table(SyncState::Table)
                .if_not_exists()
                .col(ColumnDef::new(SyncState::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(SyncState::SyncedAt).timestamp_with_time_zone().not_null())
                .to_owned(),
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(SyncState::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum SyncState {
    Table,
    Id,
    SyncedAt,
}
