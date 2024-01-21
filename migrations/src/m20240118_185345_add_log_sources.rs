use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Logs::Table)
            .drop_column(Logs::UserId)
            .add_column(ColumnDef::new(Logs::SourceId).integer().not_null())
            .add_foreign_key(TableForeignKey::new()
                .name("fk_log_source")
                .from_tbl(Logs::Table).from_col(Logs::SourceId)
                .to_tbl(Sources::Table).to_col(Sources::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Logs::Table)
            .drop_foreign_key(Alias::new("fk_log_source"))
            .drop_column(Logs::SourceId)
            .add_column(ColumnDef::new(Logs::UserId).integer().not_null())
            .add_foreign_key(TableForeignKey::new()
                .from_tbl(Logs::Table).from_col(Logs::UserId)
                .to_tbl(Users::Table).to_col(Users::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Logs {
    Table,
    SourceId,
    UserId
}

#[derive(DeriveIden)]
enum Sources {
    Table,
    Id
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id
}