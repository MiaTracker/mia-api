use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Seasons::Table)
            .add_foreign_key(TableForeignKey::new()
                .name("fk_seasons")
                .from_tbl(Seasons::Table).from_col(Seasons::SeriesId)
                .to_tbl(Series::Table).to_col(Series::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::Cascade)
            ).to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_foreign_key(ForeignKey::drop().table(Seasons::Table).name("fk_seasons").to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Series {
    Table,
    Id
}

#[derive(DeriveIden)]
enum Seasons {
    Table,
    SeriesId
}