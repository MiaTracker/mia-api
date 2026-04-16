use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Media::Table)
                .add_column(
                    ColumnDef::new(Media::OriginCountry)
                        .custom(Alias::new("TEXT[]"))
                        .null()
                )
                .to_owned(),
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(MediaLocks::Table)
                .add_column(
                    ColumnDef::new(MediaLocks::OriginCountry)
                        .boolean()
                        .not_null()
                        .default(false)
                )
                .to_owned(),
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Media::Table)
                .drop_column(Media::OriginCountry)
                .to_owned(),
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(MediaLocks::Table)
                .drop_column(MediaLocks::OriginCountry)
                .to_owned(),
        ).await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    OriginCountry,
}

#[derive(DeriveIden)]
enum MediaLocks {
    Table,
    OriginCountry,
}