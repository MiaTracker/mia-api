use log::{debug, warn};
use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::Statement;
use crate::m20260417_000003_season_episode_images::entities::images;
use crate::m20260417_000003_season_episode_images::services::save_tmdb_image;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        manager.alter_table(
            Table::alter()
                .table(Seasons::Table)
                .add_column(ColumnDef::new(Seasons::PosterImageId).integer())
                .to_owned(),
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(Episodes::Table)
                .add_column(ColumnDef::new(Episodes::StillImageId).integer())
                .to_owned(),
        ).await?;

        integrations::tmdb::services::initialize::initialize().await;

        let builder = manager.get_database_backend();

        let seasons_query = Query::select()
            .columns([Seasons::Id, Seasons::PosterPath])
            .from(Seasons::Table)
            .cond_where(Expr::col(Seasons::PosterPath).is_not_null())
            .to_owned();

        let season_rows = db.query_all(builder.build(&seasons_query)).await?;
        for row in season_rows {
            let id = row.try_get_by_index::<i32>(0)?;
            let poster_path = row.try_get_by_index::<String>(1)?;

            debug!("Migrating season {} poster image", id);

            let result: Result<(), DbErr> = async {
                let image_id = save_tmdb_image(poster_path.as_str(), images::ImageType::Poster, db).await?;
                let update = Query::update()
                    .table(Seasons::Table)
                    .values([(Seasons::PosterImageId, image_id.into())])
                    .cond_where(Expr::col(Seasons::Id).eq(id))
                    .to_owned();
                db.execute(builder.build(&update)).await?;
                Ok(())
            }.await;

            match result {
                Ok(_) => {}
                Err(e) => {
                    warn!("Failed to save poster image for season {}: {}", id, e);
                }
            }
        }

        let episodes_query = Query::select()
            .columns([Episodes::Id, Episodes::StillPath])
            .from(Episodes::Table)
            .cond_where(Expr::col(Episodes::StillPath).is_not_null())
            .to_owned();

        let episode_rows = db.query_all(builder.build(&episodes_query)).await?;
        for row in episode_rows {
            let id = row.try_get_by_index::<i32>(0)?;
            let still_path = row.try_get_by_index::<String>(1)?;

            debug!("Migrating episode {} still image", id);

            let result: Result<(), DbErr> = async {
                let image_id = save_tmdb_image(still_path.as_str(), images::ImageType::Still, db).await?;
                let update = Query::update()
                    .table(Episodes::Table)
                    .values([(Episodes::StillImageId, image_id.into())])
                    .cond_where(Expr::col(Episodes::Id).eq(id))
                    .to_owned();
                db.execute(builder.build(&update)).await?;
                Ok(())
            }.await;

            match result {
                Ok(_) => {}
                Err(e) => {
                    warn!("Failed to save still image for episode {}: {}", id, e);
                }
            }
        }

        manager.alter_table(
            Table::alter()
                .table(Seasons::Table)
                .drop_column(Seasons::PosterPath)
                .to_owned(),
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(Episodes::Table)
                .drop_column(Episodes::StillPath)
                .to_owned(),
        ).await?;

        manager.create_foreign_key(
            ForeignKey::create()
                .from(Seasons::Table, Seasons::PosterImageId)
                .to(Images::Table, Images::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::Cascade)
                .to_owned(),
        ).await?;

        manager.create_foreign_key(
            ForeignKey::create()
                .from(Episodes::Table, Episodes::StillImageId)
                .to(Images::Table, Images::Id)
                .on_update(ForeignKeyAction::Cascade)
                .on_delete(ForeignKeyAction::Cascade)
                .to_owned(),
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.alter_table(
            Table::alter()
                .table(Seasons::Table)
                .add_column(ColumnDef::new(Seasons::PosterPath).string())
                .to_owned(),
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(Episodes::Table)
                .add_column(ColumnDef::new(Episodes::StillPath).text())
                .to_owned(),
        ).await?;

        let db = manager.get_connection();
        let builder = manager.get_database_backend();

        let restore_seasons = Statement::from_string(
            builder,
            "UPDATE seasons SET poster_path = images.source_path FROM images \
             WHERE seasons.poster_image_id = images.id AND images.source_path IS NOT NULL"
                .to_string(),
        );
        db.execute(restore_seasons).await?;

        let restore_episodes = Statement::from_string(
            builder,
            "UPDATE episodes SET still_path = images.source_path FROM images \
             WHERE episodes.still_image_id = images.id AND images.source_path IS NOT NULL"
                .to_string(),
        );
        db.execute(restore_episodes).await?;

        manager.alter_table(
            Table::alter()
                .table(Seasons::Table)
                .drop_column(Seasons::PosterImageId)
                .to_owned(),
        ).await?;

        manager.alter_table(
            Table::alter()
                .table(Episodes::Table)
                .drop_column(Episodes::StillImageId)
                .to_owned(),
        ).await
    }
}

#[derive(DeriveIden)]
enum Seasons {
    Table,
    Id,
    PosterPath,
    PosterImageId,
}

#[derive(DeriveIden)]
enum Episodes {
    Table,
    Id,
    StillPath,
    StillImageId,
}

#[derive(DeriveIden)]
enum Images {
    Table,
    Id,
}

mod entities {
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
        pub enum Relation {}

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
            #[sea_orm(string_value = "still")]
            Still,
        }

        impl ImageFileType {
            pub fn to_extension(&self) -> &'static str {
                match self {
                    ImageFileType::Jpeg => "jpg",
                    ImageFileType::Png => "png",
                    ImageFileType::WebP => "webp",
                }
            }
        }
    }

    pub(crate) mod image_sizes {
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
    use std::io::Cursor;
    use std::path::{Path, PathBuf};
    use async_scoped::TokioScope;
    use image::{DynamicImage, ImageFormat, ImageReader};
    use image::imageops::FilterType;
    use log::{debug, error, warn};
    use sea_orm::{ActiveModelTrait, ColumnTrait, DbErr, EntityTrait, NotSet, PaginatorTrait, QueryFilter, Set};
    use sea_orm_migration::SchemaManagerConnection;
    use tokio::fs;
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;
    use uuid::Uuid;
    use infrastructure::config;
    use integrations::tmdb;
    use crate::m20260417_000003_season_episode_images::entities::{image_sizes, images};
    use crate::m20260417_000003_season_episode_images::entities::images::{ImageFileType, ImageSource, ImageType};

    pub async fn save_tmdb_image(tmdb_path: &str, r#type: ImageType, db: &SchemaManagerConnection<'_>) -> Result<i32, DbErr> {
        let existing = images::Entity::find()
            .filter(images::Column::SourcePath.eq(tmdb_path))
            .one(db)
            .await?;

        if let Some(existing_image) = existing {
            return Ok(existing_image.id);
        }

        let image_bytes = tmdb::services::images::image(tmdb_path).await
            .map_err(|e| DbErr::Custom(format!("Error during tmdb call {}", e)))?;

        let reader = match ImageReader::new(Cursor::new(image_bytes)).with_guessed_format() {
            Ok(reader) => reader,
            Err(err) => {
                warn!("Failed to parse TMDB image: {}", err.to_string());
                return Err(DbErr::Custom("Failed to parse TMDB image".to_string()));
            }
        };

        let format = reader.format().unwrap_or(ImageFormat::Png);

        let image = match reader.decode() {
            Ok(img) => img,
            Err(err) => {
                warn!("Failed to decode TMDB image: {}", err.to_string());
                return Err(DbErr::Custom("Failed to decode TMDB image".to_string()));
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
            let conflicts = images::Entity::find()
                .filter(images::Column::Path.eq(&uuid))
                .count(db)
                .await?;
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
            file_type: Set(image_file_type.clone()),
        }
        .insert(db)
        .await?;

        fs::create_dir_all(&directory_path)
            .await
            .map_err(|_| DbErr::Custom("Failed to create image directory".to_string()))?;

        save_image_size(db_image.id, width, height, &directory_path, extension, format, &image, db).await?;
        save_image_sizes_task(db_image.id, width, height, directory_path, extension, format, image, db).await?;

        Ok(db_image.id)
    }

    async fn save_image_sizes_task(
        image_id: i32,
        width: u32,
        height: u32,
        directory_path: PathBuf,
        extension: &str,
        format: ImageFormat,
        image: DynamicImage,
        db: &SchemaManagerConnection<'_>,
    ) -> Result<(), DbErr> {
        let gcd = gcd::binary_u32(width, height);
        let width_short = width <= height;
        let short = if width_short { width } else { height };
        let long = if width_short { height } else { width };
        let step = short / gcd;
        let ratio = long as f32 / short as f32;

        let (_, results) = unsafe {
            TokioScope::scope_and_collect(|scope| {
                let mut i = 0;
                loop {
                    let threshold = 100 + 200 * i;
                    if threshold > short - 200 {
                        break;
                    }
                    let directory_path = &directory_path;
                    let image = &image;
                    let (resized_width, resized_height) =
                        calculate_image_size(threshold, width_short, step, ratio);
                    let db = &db;
                    scope.spawn(async move {
                        save_image_size(image_id, resized_width, resized_height, directory_path, extension, format, image, db).await
                    });
                    i += 1;
                }
            })
        }
        .await;

        let results = results
            .into_iter()
            .map(|r| {
                r.map_err(|err| {
                    error!("An error returned by Tokio while joining tasks: {:?}", err);
                    DbErr::Custom("A Tokio task failed".to_string())
                })
            })
            .collect::<Result<Vec<_>, DbErr>>();
        for result in results? {
            result?
        }
        Ok(())
    }

    fn calculate_image_size(threshold: u32, width_short: bool, step: u32, ratio: f32) -> (u32, u32) {
        let multiplier = if step == threshold || step <= 100 {
            (threshold as f32 / step as f32).ceil()
        } else {
            threshold as f32 / step as f32
        };
        let resized_short_f = step as f32 * multiplier;
        let resized_long = (resized_short_f * ratio).round() as u32;
        let resized_short = resized_short_f.round() as u32;
        if width_short {
            (resized_short, resized_long)
        } else {
            (resized_long, resized_short)
        }
    }

    async fn save_image_size(
        image_id: i32,
        width: u32,
        height: u32,
        directory_path: &PathBuf,
        extension: &str,
        format: ImageFormat,
        image: &DynamicImage,
        db: &SchemaManagerConnection<'_>,
    ) -> Result<(), DbErr> {
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

            let mut file = File::create_new(&path).await.map_err(|e| {
                error!("File {} for size {}x{} already exists. Error: {}", path.display(), width, height, e);
                DbErr::Custom("File already exists".to_string())
            })?;
            if let Err(_) = file.write_all(buffer.as_slice()).await {
                drop(file);
                let _ = fs::remove_file(path).await;
                return Err(DbErr::Custom("Error writing image file".to_string()));
            }
            file.flush()
                .await
                .map_err(|_| DbErr::Custom("Error flushing image file".to_string()))?;
        }

        image_sizes::ActiveModel {
            id: Default::default(),
            image_id: Set(image_id),
            width: Set(width as i32),
            height: Set(height as i32),
        }
        .insert(db)
        .await?;

        Ok(())
    }
}
