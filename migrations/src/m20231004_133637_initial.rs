use sea_orm_migration::prelude::*;
use crate::extension::postgres::Type;
use crate::sea_orm::EnumIter;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_type(
            Type::create()
                .as_enum(MediaType::Table)
                .values([MediaType::Movie, MediaType::Series])
                .to_owned()
        ).await?;

        manager.create_type(
            Type::create()
                .as_enum(CreditsType::Table)
                .values([CreditsType::Cast, CreditsType::Crew])
                .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(Users::Table)
            .col(ColumnDef::new(Users::Id)
                .integer()
                .not_null()
                .primary_key()
                .auto_increment())
            .col(ColumnDef::new(Users::Uuid)
                .uuid()
                .not_null()
                .unique_key())
            .col(ColumnDef::new(Users::Email)
                .string()
                .not_null()
                .unique_key())
            .col(ColumnDef::new(Users::Username)
                .string()
                .not_null()
                .unique_key())
            .col(ColumnDef::new(Users::PasswordHash)
                .string()
                .not_null())
            .col(ColumnDef::new(Users::Admin)
                .boolean()
                .not_null())
            .to_owned()
        ).await?;


        manager.create_table(Table::create()
                .table(Languages::Table)
                .col(ColumnDef::new(Languages::ISO6391)
                    .string()
                    .not_null()
                    .primary_key())
                .col(ColumnDef::new(Languages::EnglishName).string().not_null())
                .col(ColumnDef::new(Languages::Name).string().not_null())
                .to_owned()
            ).await?;

        manager.create_table(Table::create()
                .table(Media::Table)
                .col(ColumnDef::new(Media::Id)
                    .integer()
                    .not_null()
                    .primary_key()
                    .auto_increment())
                .col(ColumnDef::new(Media::BackdropPath).string())
                .col(ColumnDef::new(Media::Homepage).string())
                .col(ColumnDef::new(Media::TMDBId).integer())
                .col(ColumnDef::new(Media::IMDBId).string())
                .col(ColumnDef::new(Media::Overview).string())
                .col(ColumnDef::new(Media::PosterPath).string())
                .col(ColumnDef::new(Media::TMDBVoteAverage).float())
                .col(ColumnDef::new(Media::OriginalLanguage).string())
                .col(ColumnDef::new(Media::DateAdded).date().not_null())
                .col(ColumnDef::new(Media::Type).enumeration(MediaType::Table, [MediaType::Movie, MediaType::Series]).not_null())
                .index(Index::create().name("tmdbid_unique_constr").col(Media::TMDBId).col(Media::Type).unique())
                .foreign_key(ForeignKey::create().from(Media::Table, Media::OriginalLanguage).to(Languages::Table, Languages::ISO6391)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
            ).await?;


        manager.create_table(Table::create()
                .table(UserMedia::Table)
                .col(ColumnDef::new(UserMedia::MediaId)
                    .integer()
                    .not_null())
                .col(ColumnDef::new(UserMedia::UserId)
                    .integer()
                    .not_null())
                .col(ColumnDef::new(UserMedia::Stars).float())
                .primary_key(Index::create().table(UserMedia::Table)
                    .col(UserMedia::MediaId)
                    .col(UserMedia::UserId))
                .foreign_key(ForeignKey::create().from(UserMedia::Table, UserMedia::UserId).to(Users::Table, Users::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .foreign_key(ForeignKey::create().from(UserMedia::Table, UserMedia::MediaId).to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
            ).await?;

        manager.create_table(Table::create()
                .table(Movies::Table)
                .col(ColumnDef::new(Movies::Id).integer().not_null().primary_key())
                .col(ColumnDef::new(Movies::ReleaseDate).date().not_null())
                .col(ColumnDef::new(Movies::Runtime).integer())
                .col(ColumnDef::new(Movies::Status).string().not_null())
                .foreign_key(ForeignKey::create().from(Movies::Table, Movies::Id).to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
            ).await?;

        manager.create_table(Table::create()
                .table(Series::Table)
                .col(ColumnDef::new(Series::Id).integer().not_null().primary_key())
                .col(ColumnDef::new(Series::FirstAirDate).date())
                .col(ColumnDef::new(Series::NumberOfEpisodes).integer())
                .col(ColumnDef::new(Series::NumberOfSeasons).integer())
                .col(ColumnDef::new(Series::Status).string().not_null())
                .col(ColumnDef::new(Series::Type).string())
                .foreign_key(ForeignKey::create().from(Series::Table, Series::Id).to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
            ).await?;

        manager.create_table(Table::create()
                .table(Seasons::Table)
                .col(ColumnDef::new(Seasons::Id).integer().not_null().primary_key().auto_increment())
                .col(ColumnDef::new(Seasons::SeriesId).integer().not_null())
                .col(ColumnDef::new(Seasons::AirDate).date())
                .col(ColumnDef::new(Seasons::EpisodeCount).integer())
                .col(ColumnDef::new(Seasons::Name).string())
                .col(ColumnDef::new(Seasons::Overview).string())
                .col(ColumnDef::new(Seasons::PosterPath).string())
                .col(ColumnDef::new(Seasons::SeasonNumber).integer())
                .col(ColumnDef::new(Seasons::TMDBVoteAverage).float())
                .col(ColumnDef::new(Seasons::Stars).float())
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
                .table(Genres::Table)
                .col(ColumnDef::new(Genres::Id).integer().not_null().primary_key().auto_increment())
                .col(ColumnDef::new(Genres::TMDBId).integer().unique_key())
                .col(ColumnDef::new(Genres::Name).string().not_null().unique_key())
                .to_owned()
            ).await?;

        manager.create_table(Table::create()
                .table(MediaGenres::Table)
                .col(ColumnDef::new(MediaGenres::MediaId).integer().not_null())
                .col(ColumnDef::new(MediaGenres::GenreId).integer().not_null())
                .primary_key(Index::create().col(MediaGenres::MediaId).col(MediaGenres::GenreId))
                .foreign_key(ForeignKey::create().from(MediaGenres::Table, MediaGenres::MediaId).to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .foreign_key(ForeignKey::create().from(MediaGenres::Table, MediaGenres::GenreId).to(Genres::Table, Genres::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
            ).await?;

        manager.create_table(Table::create()
                .table(Titles::Table)
                .col(ColumnDef::new(Titles::Id).integer().not_null().auto_increment().primary_key())
                .col(ColumnDef::new(Titles::MediaId).integer().not_null())
                .col(ColumnDef::new(Titles::Primary).boolean().not_null())
                .col(ColumnDef::new(Titles::Title).string().not_null())
                .foreign_key(ForeignKey::create().from(Titles::Table, Titles::MediaId).to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
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
            .table(Tags::Table)
            .col(ColumnDef::new(Tags::Id).integer().not_null().auto_increment().primary_key())
            .col(ColumnDef::new(Tags::Name).string().unique_key().not_null())
            .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(MediaTags::Table)
            .col(ColumnDef::new(MediaTags::MediaId).integer().not_null())
            .col(ColumnDef::new(MediaTags::TagId).integer().not_null())
            .col(ColumnDef::new(MediaTags::UserId).integer().not_null())
            .primary_key(Index::create().col(MediaTags::MediaId).col(MediaTags::TagId).col(MediaTags::UserId))
            .foreign_key(ForeignKey::create().from(MediaTags::Table, MediaTags::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .foreign_key(ForeignKey::create().from(MediaTags::Table, MediaTags::TagId).to(Tags::Table, Tags::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .foreign_key(ForeignKey::create().from(MediaTags::Table, MediaTags::UserId).to(Users::Table, Users::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade)
            )
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
        ).await?;

        manager.create_table(Table::create()
            .table(Logs::Table)
            .col(ColumnDef::new(Logs::Id).integer().not_null().primary_key().auto_increment())
            .col(ColumnDef::new(Logs::MediaId).integer().not_null())
            .col(ColumnDef::new(Logs::Date).date().not_null())
            .col(ColumnDef::new(Logs::Rating).float())
            .col(ColumnDef::new(Logs::Completed).boolean().not_null())
            .col(ColumnDef::new(Logs::Comment).string())
            .foreign_key(ForeignKey::create().from(Logs::Table, Logs::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Logs::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(SeriesWatchlist::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Watchlist::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Credits::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(People::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(MediaTags::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Tags::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Images::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Titles::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(MediaGenres::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Genres::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Episodes::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Seasons::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Series::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Movies::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(UserMedia::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Media::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Languages::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Users::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(CreditsType::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(MediaType::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Uuid,
    Email,
    Username,
    PasswordHash,
    Admin
}

#[derive(DeriveIden)]
enum Languages {
    Table,
    ISO6391,
    EnglishName,
    Name
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    BackdropPath,
    Homepage,
    TMDBId,
    IMDBId,
    Overview,
    PosterPath,
    TMDBVoteAverage,
    OriginalLanguage,
    DateAdded,
    Type
}

#[derive(DeriveIden)]
enum UserMedia {
    Table,
    MediaId,
    UserId,
    Stars
}

#[derive(DeriveIden)]
enum Movies {
    Table,
    Id,
    ReleaseDate,
    Runtime,
    Status
}

#[derive(DeriveIden)]
enum Series {
    Table,
    Id,
    FirstAirDate,
    NumberOfEpisodes,
    NumberOfSeasons,
    Status,
    Type
}


#[derive(DeriveIden)]
enum Seasons {
    Table,
    Id,
    SeriesId,
    AirDate,
    EpisodeCount,
    Name,
    Overview,
    PosterPath,
    SeasonNumber,
    TMDBVoteAverage,
    Stars
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
enum Genres {
    Table,
    Id,
    TMDBId,
    Name
}

#[derive(DeriveIden)]
enum MediaGenres {
    Table,
    MediaId,
    GenreId
}

#[derive(DeriveIden)]
enum Titles {
    Table,
    Id,
    MediaId,
    Primary,
    Title
}

#[derive(DeriveIden)]
enum Images {
    Table,
    Id,
    MediaId,
    Path
}

#[derive(DeriveIden)]
enum Tags {
    Table,
    Id,
    Name
}

#[derive(DeriveIden)]
enum MediaTags {
    Table,
    MediaId,
    TagId,
    UserId
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
enum Credits {
    Table,
    MediaId,
    PersonId,
    Type
}

#[derive(DeriveIden)]
enum Watchlist {
    Table,
    Id,
    UserId,
    MediaId,
    Assessment,
    DateAdded
}

#[derive(DeriveIden)]
enum SeriesWatchlist {
    Table,
    Id,
    SeasonNumber
}

#[derive(DeriveIden)]
enum Logs {
    Table,
    Id,
    MediaId,
    Date,
    Rating,
    Completed,
    Comment
}

#[derive(Iden, EnumIter)]
enum CreditsType {
    Table,
    Cast,
    Crew
}

#[derive(Iden, EnumIter)]
enum MediaType {
    Table,
    Movie,
    Series
}