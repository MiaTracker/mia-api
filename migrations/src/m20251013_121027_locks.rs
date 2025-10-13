use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {

        manager.create_table(
            Table::create()
                .table(MediaLocks::Table)
                .if_not_exists()
                .col(ColumnDef::new(MediaLocks::MediaId).integer().primary_key())
                .col(ColumnDef::new(MediaLocks::BackdropPath).boolean().not_null())
                .col(ColumnDef::new(MediaLocks::Homepage).boolean().not_null())
                .col(ColumnDef::new(MediaLocks::ImdbId).boolean().not_null())
                .col(ColumnDef::new(MediaLocks::Overview).boolean().not_null())
                .col(ColumnDef::new(MediaLocks::PosterPath).boolean().not_null())
                .col(ColumnDef::new(MediaLocks::TmdbVoteAverage).boolean().not_null())
                .col(ColumnDef::new(MediaLocks::OriginalLanguage).boolean().not_null())
                .foreign_key(ForeignKey::create()
                    .from(MediaLocks::Table, MediaLocks::MediaId)
                    .to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned(),
        )
        .await?;

        manager.create_table(
            Table::create()
                .table(MovieLocks::Table)
                .if_not_exists()
                .col(ColumnDef::new(MovieLocks::MovieId).integer().primary_key())
                .col(ColumnDef::new(MovieLocks::ReleaseDate).boolean().not_null())
                .col(ColumnDef::new(MovieLocks::Runtime).boolean().not_null())
                .col(ColumnDef::new(MovieLocks::Status).boolean().not_null())
                .foreign_key(ForeignKey::create()
                    .from(MovieLocks::Table, MovieLocks::MovieId)
                    .to(Movies::Table, Movies::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(SeriesLocks::Table)
                .if_not_exists()
                .col(ColumnDef::new(SeriesLocks::SeriesId).integer().primary_key())
                .col(ColumnDef::new(SeriesLocks::FirstAirDate).boolean().not_null())
                .col(ColumnDef::new(SeriesLocks::NumberOfEpisodes).boolean().not_null())
                .col(ColumnDef::new(SeriesLocks::NumberOfSeasons).boolean().not_null())
                .col(ColumnDef::new(SeriesLocks::Status).boolean().not_null())
                .col(ColumnDef::new(SeriesLocks::Type).boolean().not_null())
                .foreign_key(ForeignKey::create()
                    .from(SeriesLocks::Table, SeriesLocks::SeriesId)
                    .to(Series::Table, Series::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(MediaLocks::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(MovieLocks::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SeriesLocks::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id
}

#[derive(DeriveIden)]
enum Movies {
    Table,
    Id
}

#[derive(DeriveIden)]
enum Series {
    Table,
    Id
}

#[derive(DeriveIden)]
enum MediaLocks {
    Table,
    MediaId,
    BackdropPath,
    Homepage,
    ImdbId,
    Overview,
    PosterPath,
    TmdbVoteAverage,
    OriginalLanguage
}

#[derive(DeriveIden)]
enum MovieLocks {
    Table,
    MovieId,
    ReleaseDate,
    Runtime,
    Status
}

#[derive(DeriveIden)]
enum SeriesLocks {
    Table,
    SeriesId,
    FirstAirDate,
    NumberOfEpisodes,
    NumberOfSeasons,
    Status,
    Type
}