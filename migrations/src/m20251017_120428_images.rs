use log::debug;
use sea_orm_migration::prelude::*;
use crate::extension::postgres::Type;
use crate::sea_orm::EnumIter;
use crate::m20251017_120428_images::entities::images;
use crate::m20251017_120428_images::services::save_tmdb_image;

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

        integrations::tmdb::services::initialize::initialize().await;
        let res = db.query_all(builder.build(&query)).await?;
        for row in res {
            let id = row.try_get_by_index::<i32>(0)?;
            let backdrop_path = row.try_get_by_index_nullable::<Option<String>>(1)?;
            let poster_path = row.try_get_by_index_nullable::<Option<String>>(2)?;

            debug!("Migrating media {}", id);

            if let Some(backdrop_path) = backdrop_path {
                let backdrop_id = save_tmdb_image(backdrop_path.as_str(), images::ImageType::Backdrop,
                                                  db).await.map_err(|_| DbErr::Migration("Error during tmdb image saving".to_string()))?;


                let update = Query::update()
                    .table(Media::Table)
                    .values([(Media::BackdropImageId, backdrop_id.into())])
                    .cond_where(Expr::col(Media::Id).eq(id))
                    .to_owned();

                db.execute(builder.build(&update)).await?;
            }

            if let Some(poster_path) = poster_path {
                let poster_id = save_tmdb_image(poster_path.as_str(), images::ImageType::Poster,
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

mod entities {
    pub(crate) mod media {
        //! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.17

        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
        #[sea_orm(table_name = "media")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i32,
            pub backdrop_path: Option<String>,
            pub homepage: Option<String>,
            pub tmdb_id: Option<i32>,
            pub imdb_id: Option<String>,
            pub overview: Option<String>,
            pub poster_path: Option<String>,
            #[sea_orm(column_type = "Float", nullable)]
            pub tmdb_vote_average: Option<f32>,
            pub original_language: Option<String>,
            pub date_added: Date,
            pub r#type: MediaType,
            pub user_id: i32,
            #[sea_orm(column_type = "Float", nullable)]
            pub stars: Option<f32>,
            pub bot_controllable: bool,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
        }

        impl ActiveModelBehavior for ActiveModel {}

        #[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
        #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "media_type")]
        pub enum MediaType {
            #[sea_orm(string_value = "movie")]
            Movie,
            #[sea_orm(string_value = "series")]
            Series,
        }
    }

    pub(crate) mod images {
        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
        #[sea_orm(table_name = "images")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i32,
            pub path: String,
            pub source: ImageSource,
            pub source_path: Option<String>,
            pub r#type: ImageType,
            pub file_type: ImageFileType,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::image_sizes::Entity")]
            ImageSizes
        }

        impl Related<super::image_sizes::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::ImageSizes.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}

        #[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
        #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "image_file_type")]
        pub enum ImageFileType {
            #[sea_orm(string_value = "jpeg")]
            Jpeg,
            #[sea_orm(string_value = "png")]
            Png,
            #[sea_orm(string_value = "web_p")]
            WebP,
        }
        #[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
        #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "image_source")]
        pub enum ImageSource {
            #[sea_orm(string_value = "tmdb")]
            Tmdb,
            #[sea_orm(string_value = "manual")]
            Manual,
        }
        #[derive(Debug, Clone, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
        #[sea_orm(rs_type = "String", db_type = "Enum", enum_name = "image_type")]
        pub enum ImageType {
            #[sea_orm(string_value = "backdrop")]
            Backdrop,
            #[sea_orm(string_value = "poster")]
            Poster,
        }

        impl ImageFileType {
            pub fn to_extension(&self) -> &'static str {
                match self {
                    ImageFileType::Jpeg => "jpg",
                    ImageFileType::Png => "png",
                    ImageFileType::WebP => "webp"
                }
            }
        }
    }

    pub(crate) mod image_sizes {
        //! `SeaORM` Entity, @generated by sea-orm-codegen 1.1.17

        use sea_orm::entity::prelude::*;

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
        #[sea_orm(table_name = "image_sizes")]
        pub struct Model {
            #[sea_orm(primary_key)]
            pub id: i32,
            pub image_id: i32,
            pub width: i32,
            pub height: i32,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::images::Entity",
                from = "Column::ImageId",
                to = "super::images::Column::Id",
                on_update = "Cascade",
                on_delete = "Cascade"
            )]
            Images,
        }

        impl Related<super::images::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Images.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

mod services {
    use sea_orm::PaginatorTrait;
use sea_orm::QueryFilter;
use sea_orm::ColumnTrait;
use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use async_scoped::TokioScope;
    use image::{DynamicImage, ImageFormat, ImageReader};
    use image::imageops::FilterType;
    use log::{debug, error, warn};
    use sea_orm::{ActiveModelTrait, DbErr, EntityTrait, NotSet, Set};
    use sea_orm_migration::SchemaManagerConnection;
    use tokio::fs;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;
    use uuid::Uuid;
    use infrastructure::config;
    use integrations::tmdb;
    use crate::m20251017_120428_images::entities::{image_sizes, images};
    use crate::m20251017_120428_images::entities::images::{ImageFileType, ImageSource, ImageType};

    pub async fn save_tmdb_image(tmdb_path: &str, r#type: ImageType, db: &SchemaManagerConnection<'_>) -> Result<i32, DbErr> {
        let backdrop_bytes = tmdb::services::images::image(tmdb_path).await
            .map_err(|e| DbErr::Custom(format!("Error during tmdb call {}", e)))?;

        let reader = match ImageReader::new(Cursor::new(backdrop_bytes)).with_guessed_format() {
            Ok(reader) => reader,
            Err(err) => {
                warn!("Failed to parse TMDB image: {}", err.to_string());
                return Err(DbErr::Custom("Failed to parse TMDB image".to_string()))
            }
        };

        let format = reader.format().unwrap_or(ImageFormat::Png);

        let image = match reader.decode() {
            Ok(img) => img,
            Err(err) => {
                warn!("Failed to decode TMDB image: {}", err.to_string());
                return Err(DbErr::Custom("Failed to decode TMDB image".to_string()))
            }
        };

        let image_file_type = match format {
            ImageFormat::Png => ImageFileType::Png,
            ImageFormat::Jpeg => ImageFileType::Jpeg,
            ImageFormat::WebP => ImageFileType::WebP,
            _ => {
                warn!("TMDB image in unsupported format: {:?}", format);
                return Err(DbErr::Custom("Unsupported TMDB image format".to_string()));
            }
        };

        let extension: &'static str = image_file_type.to_extension();

        let directory = loop {
            let uuid = Uuid::new_v4().simple().to_string();

            let conflicts = images::Entity::find().filter(images::Column::Path.eq(&uuid))
                .count(db).await?;

            if conflicts == 0 {
                break uuid;
            }
        };
        let directory_path = Path::new(&config().images.store_path).join(&directory);

        let width = image.width();
        let height = image.height();

        let db_image = images::ActiveModel {
            id: NotSet,
            path: Set(directory),
            source: Set(ImageSource::Tmdb),
            source_path: Set(Some(tmdb_path.to_string())),
            r#type: Set(r#type),
            file_type: Set(image_file_type.clone())
        }.insert(db).await?;

        fs::create_dir_all(&directory_path).await.map_err(|_| DbErr::Custom("Failed to create image directory".to_string()))?;

        save_image_size(db_image.id, width, height, &directory_path, extension, format, &image, db).await?;
        save_image_sizes_task(db_image.id, width, height, directory_path, extension, format, image, db).await?;

        Ok(db_image.id)
    }

    async fn save_image_sizes_task(image_id: i32, width: u32, height: u32, directory_path: PathBuf,
                                      extension: &str, format: ImageFormat, image: DynamicImage, db: &SchemaManagerConnection<'_>) -> Result<(), DbErr> {
        let gcd = gcd::binary_u32(width, height);

        let width_short = width <= height;
        let short = if width_short { width } else { height };
        let long = if width_short { height } else { width };
        let step = short / gcd;

        let ratio = long as f32 / short as f32;

        let (_, results) = unsafe {
            // Safe because the Scope is never forgotten
            TokioScope::scope_and_collect(|scope| {
                let mut i = 0;
                loop {
                    let threshold = 100 + 200 * i;
                    if threshold > short - 200 {
                        break;
                    }
                    let directory_path = &directory_path;
                    let image = &image;
                    let (resized_width, resized_height) = calculate_image_size(threshold, width_short, step, ratio);
                    let db = &db;

                    scope.spawn(async move {
                        save_image_size(image_id, resized_width, resized_height, directory_path, extension, format, image, db).await
                    });

                    i += 1;
                }
            })
        }.await;

        let results = results.into_iter().map(|r| r.map_err(|err| {
            error!("An error returned by Tokio while joining tasks: {:?}", err);
            return DbErr::Custom("A Tokio task failed".to_string());
        })).collect::<Result<Vec<_>, DbErr>>();
        for result in results? {
            result?
        }
        Ok(())
    }

    fn calculate_image_size(threshold: u32, width_short: bool, step: u32, ratio: f32) -> (u32, u32) {
        let multiplier = if step == threshold || step <= 100 {
            (threshold as f32 / step as f32).ceil()
        } else { threshold as f32 / step as f32 };

        let resized_short_f = step as f32 * multiplier;
        let resized_long = (resized_short_f * ratio).round() as u32;
        let resized_short = resized_short_f.round() as u32;


        if width_short {
            (resized_short, resized_long)
        } else {
            (resized_long, resized_short)
        }
    }

    async fn save_image_size(image_id: i32, width: u32, height: u32, directory_path: &PathBuf,
                                extension: &str, format: ImageFormat, image: &DynamicImage, db: &SchemaManagerConnection<'_>) -> Result<(), DbErr> {

        let path = directory_path
            .join(format!("{}x{}", width, height))
            .with_extension(extension);

        {
            let resized_image = if image.width() == width && image.height() == height {
                debug!("Saving original image. Size: {} x {}", width, height);
                image.clone()
            } else {
                debug!("Started image resize. Size: {} x {}", width, height);
                let img = image.resize_exact(width, height, FilterType::Lanczos3);
                debug!("Finished image resize");
                img
            };

            let mut buffer = Vec::new();
            resized_image.write_to(Cursor::new(&mut buffer), format).map_err(|e| {
                error!("Error writing image to buffer: {}", e);
                DbErr::Custom("Failed to write image".to_string())
            })?;

            let mut file = File::create_new(&path).await.map_err(|e|
                {
                    error!("File {} for size {}x{} already exists. Error: {}", path.display(), width, height, e);
                    DbErr::Custom("File already exists".to_string())
                })?;
            if let Err(_) = file.write_all(buffer.as_slice()).await {
                drop(file);
                let _ = fs::remove_file(path).await;
                return Err(DbErr::Custom("Error writing image file".to_string()));
            }
            file.flush().await.map_err(|_| DbErr::Custom("Error flushing image file".to_string()))?;
        }

        image_sizes::ActiveModel {
            id: Default::default(),
            image_id: Set(image_id),
            width: Set(width as i32),
            height: Set(height as i32),
        }.insert(db).await?;

        Ok(())
    }
}