use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop()
            .table(SeriesWatchlist::Table)
            .to_owned()
        ).await?;
        manager.drop_table(Table::drop()
            .table(Watchlist::Table)
            .to_owned()
        ).await?;
        manager.create_table(Table::create()
            .table(Watchlist::Table)
            .col(ColumnDef::new(Watchlist::MediaId).integer().not_null().primary_key())
            .col(ColumnDef::new(Watchlist::Assessment).integer())
            .col(ColumnDef::new(Watchlist::DateAdded).date().not_null())
            .foreign_key(ForeignKey::create().from(Watchlist::Table, Watchlist::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop()
            .table(Watchlist::Table)
            .to_owned()
        ).await?;
        manager.create_table(Table::create()
            .table(Watchlist::Table)
            .col(ColumnDef::new(Watchlist::Id).integer().not_null().auto_increment().primary_key())
            .col(ColumnDef::new(Watchlist::UserId).integer().not_null())
            .col(ColumnDef::new(Watchlist::MediaId).integer().not_null())
            .col(ColumnDef::new(Watchlist::Assessment).integer().not_null())
            .col(ColumnDef::new(Watchlist::DateAdded).date().not_null())
            .foreign_key(ForeignKey::create().from(Watchlist::Table, Watchlist::UserId).to(Users::Table, Users::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .foreign_key(ForeignKey::create().from(Watchlist::Table, Watchlist::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await?;
        manager.create_table(Table::create()
            .table(SeriesWatchlist::Table)
            .col(ColumnDef::new(SeriesWatchlist::Id).integer().not_null().primary_key())
            .col(ColumnDef::new(SeriesWatchlist::SeasonNumber).integer().not_null())
            .foreign_key(ForeignKey::create().from(SeriesWatchlist::Table, SeriesWatchlist::Id).to(Watchlist::Table, Watchlist::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await
    }
}

#[derive(DeriveIden)]
enum Watchlist {
    Table,
    Id,
    UserId,
    MediaId,
    Assessment,
    DateAdded,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum SeriesWatchlist {
    Table,
    Id,
    SeasonNumber
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id
}