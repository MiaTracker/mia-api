use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.truncate_table(Table::truncate().table(AppTokens::Table).to_owned()).await?;
        manager.alter_table(Table::alter()
            .table(AppTokens::Table)
            .add_column(ColumnDef::new(AppTokens::Generated).date_time().not_null())
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(AppTokens::Table)
            .drop_column(AppTokens::Generated)
            .to_owned()
        ).await
    }
}

#[derive(DeriveIden)]
enum AppTokens {
    Table,
    Generated
}
