use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_index(Index::create()
            .name("source_name_unique_constraint")
            .table(Sources::Table)
            .col(Sources::MediaId)
            .col(Sources::Name)
            .unique()
            .to_owned()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_index(Index::drop()
            .name("source_name_unique_constraint")
            .to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Sources {
    Table,
    MediaId,
    Name
}
