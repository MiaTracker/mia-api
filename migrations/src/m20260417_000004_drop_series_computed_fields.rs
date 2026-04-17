use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Series::Table)
                .drop_column(Series::NumberOfEpisodes)
                .drop_column(Series::NumberOfSeasons)
                .to_owned(),
        ).await?;
        manager.alter_table(
            Table::alter()
                .table(SeriesLocks::Table)
                .drop_column(SeriesLocks::NumberOfEpisodes)
                .drop_column(SeriesLocks::NumberOfSeasons)
                .to_owned(),
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Series::Table)
                .add_column(ColumnDef::new(Series::NumberOfEpisodes).integer())
                .add_column(ColumnDef::new(Series::NumberOfSeasons).integer())
                .to_owned(),
        ).await?;
        manager.alter_table(
            Table::alter()
                .table(SeriesLocks::Table)
                .add_column(ColumnDef::new(SeriesLocks::NumberOfEpisodes).boolean().not_null().default(false))
                .add_column(ColumnDef::new(SeriesLocks::NumberOfSeasons).boolean().not_null().default(false))
                .to_owned(),
        ).await
    }
}

#[derive(DeriveIden)]
enum Series {
    Table,
    NumberOfEpisodes,
    NumberOfSeasons,
}

#[derive(DeriveIden)]
enum SeriesLocks {
    Table,
    NumberOfEpisodes,
    NumberOfSeasons,
}
