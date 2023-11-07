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
                .as_enum(SourceType::Table)
                .values([SourceType::Torrent, SourceType::Web, SourceType::Jellyfin])
                .to_owned()
        ).await?;

        manager.create_table(Table::create()
            .table(Sources::Table)
            .col(ColumnDef::new(Sources::Id)
                .integer()
                .not_null()
                .primary_key()
                .auto_increment())
            .col(ColumnDef::new(Sources::MediaId).integer().not_null())
            .col(ColumnDef::new(Sources::Name).string().not_null())
            .col(ColumnDef::new(Sources::Url).string().not_null())
            .col(ColumnDef::new(Sources::Type).enumeration(SourceType::Table, [SourceType::Torrent, SourceType::Web, SourceType::Jellyfin]).not_null())
            .foreign_key(ForeignKey::create().from(Sources::Table, Sources::MediaId).to(Media::Table, Media::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Sources::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(SourceType::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Sources {
    Table,
    Id,
    MediaId,
    Name,
    Url,
    Type,
}

#[derive(Iden, EnumIter)]
enum SourceType {
    Table,
    Torrent,
    Web,
    Jellyfin
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id
}