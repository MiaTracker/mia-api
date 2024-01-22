use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Logs::Table)
            .rename_column(Logs::Rating, Logs::Stars)
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Logs::Table)
            .rename_column(Logs::Stars, Logs::Rating)
            .to_owned()
        ).await
    }
}

#[derive(DeriveIden)]
enum Logs {
    Table,
    Rating,
    Stars,
}
