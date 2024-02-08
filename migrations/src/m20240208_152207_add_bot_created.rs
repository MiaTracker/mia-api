use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Media::Table)
            .add_column(ColumnDef::new(Media::BotControllable).boolean())
            .to_owned()
        ).await?;

        manager.get_connection().execute_unprepared("UPDATE media SET bot_controllable = false;").await?;

        manager.alter_table(Table::alter()
                .table(Media::Table)
                .modify_column(ColumnDef::new(Media::BotControllable).boolean().not_null())
                .to_owned()
        ).await?;

        manager.alter_table(Table::alter()
            .table(Sources::Table)
            .add_column(ColumnDef::new(Sources::BotControllable).boolean())
            .to_owned()
        ).await?;

        manager.get_connection().execute_unprepared("UPDATE sources SET bot_controllable = false;").await?;

        manager.alter_table(Table::alter()
            .table(Sources::Table)
            .modify_column(ColumnDef::new(Sources::BotControllable).boolean().not_null())
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(Table::alter()
            .table(Sources::Table)
            .drop_column(Sources::BotControllable)
            .to_owned()
        ).await?;

        manager.alter_table(Table::alter()
            .table(Media::Table)
            .drop_column(Media::BotControllable)
            .to_owned()
        ).await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    BotControllable
}

#[derive(DeriveIden)]
enum Sources {
    Table,
    BotControllable
}