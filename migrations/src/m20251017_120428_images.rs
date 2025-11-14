use sea_orm_migration::prelude::*;
use entities::sea_orm_active_enums;
use crate::extension::postgres::Type;
use crate::sea_orm::EnumIter;
use services::images::save_tmdb_image;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_type(
            Type::create()
                .as_enum(ImageSource::Table)
                .values([ImageSource::TMDB, ImageSource::Manual])
                .to_owned()
        ).await?;

        manager.create_type(
            Type::create()
                .as_enum(ImageType::Table)
                .values([ImageType::Backdrop, ImageType::Poster])
                .to_owned()
        ).await?;

        manager.create_type(
            Type::create()
                .as_enum(ImageFileType::Table)
                .values([ImageFileType::Jpeg, ImageFileType::Png, ImageFileType::WebP])
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(Images::Table)
                .col(ColumnDef::new(Images::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(Images::Path).string().not_null())
                .col(ColumnDef::new(Images::Source).enumeration(ImageSource::Table, [ImageSource::TMDB, ImageSource::Manual]).not_null())
                .col(ColumnDef::new(Images::SourcePath).string())
                .col(ColumnDef::new(Images::Type).enumeration(ImageType::Table, [ImageType::Poster, ImageType::Poster]).not_null())
                .col(ColumnDef::new(Images::FileType).enumeration(ImageFileType::Table, [ImageFileType::Jpeg, ImageFileType::Png, ImageFileType::WebP]).not_null())
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(ImageSizes::Table)
                .col(ColumnDef::new(ImageSizes::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(ImageSizes::ImageId).integer().not_null())
                .col(ColumnDef::new(ImageSizes::Width).integer().not_null())
                .col(ColumnDef::new(ImageSizes::Height).integer().not_null())
                .foreign_key(ForeignKey::create()
                    .from(ImageSizes::Table, ImageSizes::ImageId)
                    .to(Images::Table, Images::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(ManualImageReferences::Table)
                .col(ColumnDef::new(ManualImageReferences::ImageId).integer().not_null().primary_key())
                .col(ColumnDef::new(ManualImageReferences::MediaId).integer().not_null())
                .foreign_key(ForeignKey::create()
                    .from(ManualImageReferences::Table, ManualImageReferences::ImageId)
                    .to(Images::Table, Images::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .foreign_key(ForeignKey::create()
                    .from(ManualImageReferences::Table, ManualImageReferences::ImageId)
                    .to(Media::Table, Media::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(Media::Table)
                .add_column(ColumnDef::new(Media::BackdropImageId).integer())
                .add_column(ColumnDef::new(Media::PosterImageId).integer())
                .to_owned()
        ).await?;

        let db = manager.get_connection();
        let builder = manager.get_database_backend();

        let query = Query::select()
            .columns([Media::Id, Media::BackdropPath, Media::PosterPath])
            .from(Media::Table)
            .cond_where(Expr::col(Media::TmdbId).is_not_null())
            .to_owned();

        services::infrastructure::initialize().await;
        let res = db.query_all(builder.build(&query)).await?;
        for row in res {
            let id = row.try_get_by_index::<i32>(0)?;
            let backdrop_path = row.try_get_by_index_nullable::<Option<String>>(1)?;
            let poster_path = row.try_get_by_index_nullable::<Option<String>>(2)?;

            if let Some(backdrop_path) = backdrop_path {
                let backdrop_id = save_tmdb_image(backdrop_path.as_str(), sea_orm_active_enums::ImageType::Backdrop,
                                                  db).await.map_err(|_| DbErr::Migration("Error during tmdb image saving".to_string()))?;


                let update = Query::update()
                    .table(Media::Table)
                    .values([(Media::BackdropImageId, backdrop_id.into())])
                    .cond_where(Expr::col(Media::Id).eq(id))
                    .to_owned();

                db.execute(builder.build(&update)).await?;
            }

            if let Some(poster_path) = poster_path {
                let poster_id = save_tmdb_image(poster_path.as_str(), sea_orm_active_enums::ImageType::Poster,
                                                  db).await.map_err(|_| DbErr::Migration("Error during tmdb image saving".to_string()))?;


                let update = Query::update()
                    .table(Media::Table)
                    .values([(Media::PosterImageId, poster_id.into())])
                    .cond_where(Expr::col(Media::Id).eq(id))
                    .to_owned();

                db.execute(builder.build(&update)).await?;
            }
        }

        manager.create_foreign_key(
            ForeignKey::create()
                .from(Media::Table, Media::BackdropImageId)
                .to(Images::Table, Images::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade)
                .to_owned()
        ).await?;

        manager.create_foreign_key(
            ForeignKey::create()
                .from(Media::Table, Media::PosterImageId)
                .to(Images::Table, Images::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade)
                .to_owned()
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(Media::Table)
                .drop_column(Media::BackdropPath)
                .drop_column(Media::PosterPath)
                .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Media::Table)
                .add_column(ColumnDef::new(Media::BackdropPath).string())
                .add_column(ColumnDef::new(Media::PosterPath).string())
                .to_owned()
        ).await?;

        let db = manager.get_connection();
        let builder = manager.get_database_backend();

        let query = Query::select()
            .columns([Images::Id, Images::Type, Images::SourcePath])
            .from(Images::Table)
            .cond_where(all![Expr::col(Images::Source).eq(SimpleExpr::Custom("'tmdb'".to_string())), Expr::col(Images::SourcePath).is_not_null()])
            .to_owned();

        let res = db.query_all(builder.build(&query)).await?;

        for row in res {
            let id = row.try_get_by_index::<i32>(0)?;
            let image_type = row.try_get_by_index::<String>(1)?;
            let source_path = row.try_get_by_index::<String>(2)?;

            let update = Query::update()
                .table(Media::Table)
                .values([(match image_type.as_str() {
                    "backdrop" => Media::BackdropPath,
                    "poster" => Media::PosterPath,
                    _ => panic!("Invalid enum value")
                }, source_path.into())])
                .cond_where(Expr::col(match image_type.as_str() {
                    "backdrop" => Media::BackdropPath,
                    "poster" => Media::PosterPath,
                    _ => panic!("Invalid enum value")
                }).eq(id))
                .to_owned();

            db.execute(builder.build(&update)).await?;
        }

        manager.alter_table(
            Table::alter()
                .table(Media::Table)
                .drop_column(Media::BackdropImageId)
                .drop_column(Media::PosterImageId)
                .to_owned()
        ).await?;

        manager.drop_table(Table::drop().table(ManualImageReferences::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(ImageSizes::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Images::Table).to_owned()).await?;

        manager.drop_type(Type::drop().name(ImageSource::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(ImageType::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Images {
    Table,
    Id,
    Path,
    Source,
    SourcePath,
    Type,
    FileType
}

#[derive(DeriveIden)]
enum ImageSizes {
    Table,
    Id,
    ImageId,
    Width,
    Height
}

#[derive(DeriveIden)]
enum ManualImageReferences {
    Table,
    ImageId,
    MediaId
}

#[derive(DeriveIden)]
enum Media {
    Table,
    Id,
    TmdbId,
    BackdropPath,
    PosterPath,
    BackdropImageId,
    PosterImageId
}

#[derive(Iden, EnumIter)]
enum ImageSource {
    Table,
    TMDB,
    Manual
}

#[derive(Iden, EnumIter)]
enum ImageType {
    Table,
    Backdrop,
    Poster
}

#[derive(Iden, EnumIter)]
enum ImageFileType {
    Table,
    Jpeg,
    Png,
    WebP
}