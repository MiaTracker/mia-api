use sea_orm_migration::prelude::*;
use crate::sea_orm::{DatabaseBackend, EnumIter, Statement};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(UserMedia::Table).to_owned()).await?;
        manager.alter_table(Table::alter()
            .table(Media::Table)
            .add_column(ColumnDef::new(Media::UserId).integer().not_null())
            .add_column(ColumnDef::new(Media::Stars).float())
            .add_foreign_key(TableForeignKey::new()
                .name("FK_UserMedia")
                .from_tbl(Media::Table)
                .from_col(Media::UserId)
                .to_tbl(Users::Table)
                .to_col(Users::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::Cascade)
            ).to_owned()).await?;
        manager.alter_table(Table::alter()
            .table(Genres::Table)
            .add_column(ColumnDef::new(Genres::Type)
                .enumeration(MediaType::Table, [MediaType::Movie, MediaType::Series]).not_null())
            .to_owned()).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE genres DROP CONSTRAINT genres_tmdb_id_key;")).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE genres DROP CONSTRAINT genres_name_key;")).await?;
        manager.create_index(Index::create()
            .name("genre_tmdb_id_unique_constr")
            .table(Genres::Table)
            .col(Genres::TmdbId)
            .col(Genres::Type)
            .unique()
            .to_owned()
        ).await?;
        manager.create_index(Index::create()
            .name("genre_name_unique_constr")
            .table(Genres::Table)
            .col(Genres::Name)
            .col(Genres::Type)
            .unique()
            .to_owned()
        ).await?;
        manager.alter_table(Table::alter()
            .table(MediaTags::Table)
            .drop_column(MediaTags::UserId)
            .to_owned()).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE media_tags ADD PRIMARY KEY (media_id, tag_id);")).await.map(|_| { () })
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE media_tags DROP PRIMARY KEY;")).await?;
        manager.alter_table(Table::alter()
            .table(MediaTags::Table)
            .add_column(ColumnDef::new(MediaTags::UserId).integer().not_null())
            .add_foreign_key(TableForeignKey::new()
                .from_tbl(MediaTags::Table)
                .from_col(MediaTags::UserId)
                .to_tbl(Users::Table)
                .to_col(Users::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::Cascade))
            .to_owned()).await?;
        manager.get_connection().execute(Statement::from_string(DatabaseBackend::Postgres, "ALTER TABLE media_tags ADD PRIMARY KEY (media_id, user_id, tag_id);")).await?;
        manager.drop_index(Index::drop()
            .table(Genres::Table)
            .name("genre_name_unique_constr")
            .to_owned()
        ).await?;
        manager.drop_index(Index::drop()
            .table(Genres::Table)
            .name("genre_tmdb_id_unique_constr")
            .to_owned()
        ).await?;
        manager.create_index(Index::create().name("genres_tmdb_id_key").table(Genres::Table).col(Genres::TmdbId).unique().to_owned()).await?;
        manager.alter_table(Table::alter()
            .table(Genres::Table)
            .drop_column(Genres::Type)
            .to_owned()).await?;
        manager.alter_table(Table::alter()
            .table(Media::Table)
            .drop_foreign_key(Alias::new("FK_UserMedia"))
            .drop_column(Media::Stars)
            .drop_column(Media::UserId)
            .to_owned()).await?;
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
        ).await
    }
}

#[derive(DeriveIden)]
enum UserMedia {
    Table,
    MediaId,
    UserId,
    Stars
}
#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    UserId,
    Stars
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Genres {
    Table,
    Name,
    TmdbId,
    Type
}

#[derive(DeriveIden)]
enum MediaTags {
    Table,
    UserId
}

#[derive(Iden, EnumIter)]
enum MediaType {
    Table,
    Movie,
    Series
}