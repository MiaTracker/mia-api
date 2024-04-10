use sea_orm_migration::prelude::*;
use crate::extension::postgres::Type;
use crate::sea_orm::EnumIter;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Credits::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(People::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Images::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Episodes::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(CreditsType::Table).to_owned()).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_type(
            Type::create()
                .as_enum(CreditsType::Table)
                .values([CreditsType::Cast, CreditsType::Crew])
                .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(Episodes::Table)
            .col(ColumnDef::new(Episodes::Id).integer().not_null().auto_increment().primary_key())
            .col(ColumnDef::new(Episodes::SeasonId).integer().not_null())
            .col(ColumnDef::new(Episodes::AirDate).date())
            .col(ColumnDef::new(Episodes::EpisodeNumber).integer().not_null())
            .col(ColumnDef::new(Episodes::Name).string())
            .col(ColumnDef::new(Episodes::Overview).string())
            .col(ColumnDef::new(Episodes::Runtime).string())
            .foreign_key(ForeignKey::create().from(Episodes::Table, Episodes::SeasonId).to(Seasons::Table, Seasons::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .index(Index::create().name("episodes_unique_constr").col(Episodes::SeasonId).col(Episodes::EpisodeNumber).unique())
            .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(Images::Table)
            .col(ColumnDef::new(Images::Id).integer().not_null().auto_increment().primary_key())
            .col(ColumnDef::new(Images::MediaId).integer().not_null())
            .col(ColumnDef::new(Images::Path).string().not_null())
            .foreign_key(ForeignKey::create().from(Images::Table, Images::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .index(Index::create().name("images_unique_constr").table(Images::Table).col(Images::MediaId).col(Images::Path).unique())
            .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(People::Table)
            .col(ColumnDef::new(People::Id).integer().not_null().primary_key().auto_increment())
            .col(ColumnDef::new(People::TMDBId).integer().unique_key().not_null())
            .col(ColumnDef::new(People::IMDBId).string())
            .col(ColumnDef::new(People::Name).string().not_null())
            .col(ColumnDef::new(People::AlsoKnownAs).array(ColumnType::String(None).to_owned()))
            .col(ColumnDef::new(People::Biography).string())
            .col(ColumnDef::new(People::BirthDay).date())
            .col(ColumnDef::new(People::DeathDay).date())
            .col(ColumnDef::new(People::Gender).integer().not_null())
            .col(ColumnDef::new(People::Homepage).string())
            .col(ColumnDef::new(People::PlaceOfBirth).string())
            .col(ColumnDef::new(People::ProfilePath).string())
            .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(Credits::Table)
            .col(ColumnDef::new(Credits::MediaId).integer().not_null())
            .col(ColumnDef::new(Credits::PersonId).integer().not_null())
            .col(ColumnDef::new(Credits::Type).enumeration(CreditsType::Table, [CreditsType::Cast, CreditsType::Crew]).not_null())
            .primary_key(Index::create().col(Credits::MediaId).col(Credits::PersonId).col(Credits::Type))
            .foreign_key(ForeignKey::create().from(Credits::Table, Credits::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .foreign_key(ForeignKey::create().from(Credits::Table, Credits::PersonId).to(People::Table, People::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await
    }
}

#[derive(Iden, EnumIter)]
enum CreditsType {
    Table,
    Cast,
    Crew
}

#[derive(DeriveIden)]
enum SeriesWatchlist {
    Table,
    Id,
    SeasonNumber
}

#[derive(DeriveIden)]
enum Credits {
    Table,
    MediaId,
    PersonId,
    Type
}

#[derive(DeriveIden)]
enum People {
    Table,
    Id,
    TMDBId,
    IMDBId,
    Name,
    AlsoKnownAs,
    Biography,
    BirthDay,
    DeathDay,
    Gender,
    Homepage,
    PlaceOfBirth,
    ProfilePath
}

#[derive(DeriveIden)]
enum Images {
    Table,
    Id,
    MediaId,
    Path
}

#[derive(DeriveIden)]
enum Episodes {
    Table,
    Id,
    SeasonId,
    AirDate,
    EpisodeNumber,
    Name,
    Overview,
    Runtime
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id
}

#[derive(DeriveIden)]
enum Seasons {
    Table,
    Id
}