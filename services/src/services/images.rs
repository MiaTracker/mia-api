use std::io::{Cursor, ErrorKind};
use std::path::{Path, PathBuf};
use async_scoped::TokioScope;
use bytes::Bytes;
use fancy_regex::Regex;
use futures::{Stream, TryStreamExt};
use http::StatusCode;
use image::{DynamicImage, ImageFormat, ImageReader};
use image::imageops::FilterType;
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbConn, EntityTrait, ModelTrait, NotSet, Set};
use tokio::fs;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;
use entities::{image_sizes, images, manual_image_references, media};
use entities::prelude::{ImageSizes, Images, ManualImageReferences, Media};
use entities::sea_orm_active_enums::{ImageFileType, ImageSource, ImageType};
use infrastructure::config;
use integrations::tmdb;
use views::images::Image;
use crate::infrastructure::SrvErr;
use sea_orm::QueryFilter;
use sea_orm::PaginatorTrait;

static SLUG_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^\\d+x\\d+$").expect("Invalid slug regex")
});

static DIRECTORY_NAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^[0-9abcdef]{32}$").expect("Invalid directory name regex")
});

pub async fn get_local(slug: String, name: String) -> Result<(&'static str, ReaderStream<File>), SrvErr> {
    let path = Path::new(name.as_str());

    let directory_name = path.file_stem().ok_or(SrvErr::BadRequest("File name invalid".to_string()))?;

    match DIRECTORY_NAME_REGEX.is_match(directory_name.to_str()
        .ok_or(SrvErr::BadRequest("File name invalid".to_string()))?) {
        Ok(m) => {
            if !m {
                return Err(SrvErr::BadRequest("File name invalid".to_string()))
            }
        }
        Err(_) => return Err(SrvErr::BadRequest("File name invalid".to_string()))
    }

    match SLUG_REGEX.is_match(slug.as_str()) {
        Ok(m) => {
            if !m {
                return Err(SrvErr::BadRequest("Slug invalid".to_string()))
            }
        }
        Err(_) => return Err(SrvErr::BadRequest("Slug invalid".to_string()))
    }

    let extension = path.extension().ok_or(SrvErr::BadRequest("File name invalid".to_string()))?;
    let file_type = ImageFileType::try_from_extension(&extension.to_str()
        .ok_or(SrvErr::BadRequest("File name invalid".to_string()))?).ok_or(
        SrvErr::BadRequest("File name invalid".to_string())
    )?;

    let content_type = match file_type {
        ImageFileType::Jpeg => ImageFormat::Jpeg,
        ImageFileType::Png => ImageFormat::Png,
        ImageFileType::WebP => ImageFormat::WebP
    }.to_mime_type();

    let image_path = Path::new(&config().images.store_path).join(directory_name)
        .join(slug).with_extension(extension);

    let file = match File::open(&image_path).await {
        Ok(file) => file,
        Err(err) => {
            return match err.kind() {
                ErrorKind::NotFound => Err(SrvErr::NotFound),
                _ => {
                    error!("Error reading image file {}: {}", image_path.display(), err.to_string());
                    Err(SrvErr::Internal("Error reading image file".to_string()))
                }
            }
        }
    };

    let stream = ReaderStream::new(file);

    Ok((content_type, stream))
}

pub async fn get_tmdb(slug: String, path: String) -> Result<(StatusCode, String, impl Stream<Item=Result<Bytes, std::io::Error>>), SrvErr> {
    let path = format!("/{}", path);
    let (status_code, content_type, stream) = tmdb::services::images::image_stream(slug, path).await?;
    if let Some(stream) = stream {
        let stream = stream.map_err(|e| e.into());
        Ok((status_code, content_type, stream))
    } else {
        Err(match status_code {
            StatusCode::NOT_FOUND => SrvErr::NotFound,
            StatusCode::BAD_REQUEST => SrvErr::NotFound,
            _ => {
                warn!("Image request to TMDB failed with status code {}", status_code);
                SrvErr::Integration("Request to integration resource failed.".to_string())
            }
        })
    }
}

pub async fn save_tmdb_image<C>(tmdb_path: &str, r#type: ImageType, db: &C) -> Result<i32, SrvErr> where C : ConnectionTrait {
    let backdrop_bytes = tmdb::services::images::image(tmdb_path).await?;

    let reader = match ImageReader::new(Cursor::new(backdrop_bytes)).with_guessed_format() {
        Ok(reader) => reader,
        Err(err) => {
            warn!("Failed to parse TMDB image: {}", err.to_string());
            return Err(SrvErr::Integration("Failed to parse TMDB image".to_string()))
        }
    };

    let format = reader.format().unwrap_or(ImageFormat::Png);

    let image = match reader.decode() {
        Ok(img) => img,
        Err(err) => {
            warn!("Failed to decode TMDB image: {}", err.to_string());
            return Err(SrvErr::Integration("Failed to decode TMDB image".to_string()))
        }
    };

    let image_file_type = match format {
        ImageFormat::Png => ImageFileType::Png,
        ImageFormat::Jpeg => ImageFileType::Jpeg,
        ImageFormat::WebP => ImageFileType::WebP,
        _ => {
            warn!("TMDB image in unsupported format: {:?}", format);
            return Err(SrvErr::Integration("Unsupported TMDB image format".to_string()));
        }
    };

    let extension = image_file_type.to_extension();

    let directory = loop {
        let uuid = Uuid::new_v4().simple().to_string();

        let conflicts = Images::find().filter(images::Column::Path.eq(&uuid))
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

    fs::create_dir_all(&directory_path).await?;

    save_image_size(db_image.id, width, height, &directory_path, extension, format, &image, db).await?;

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

                scope.spawn(async move {
                    save_image_size(db_image.id, resized_width, resized_height, directory_path, extension, format, image, db).await
                });

                i += 1;
            }
        })
    }.await;


    let results = results.into_iter().map(|r| r.map_err(|err| {
        error!("An error returned by Tokio while joining tasks: {:?}", err);
        return SrvErr::Internal("A Tokio task failed".to_string());
    })).collect::<Result<Vec<_>, SrvErr>>()?;
    for result in results {
        result?;
    }

    Ok(db_image.id)
}

pub(crate) async fn fetch_backdrop_and_poster(db_media: &media::Model, db: &DbConn) -> Result<(Option<Image>, Option<Image>), SrvErr> {
    let images_with_sizes = Images::find().filter(images::Column::Id.is_in(
        vec![db_media.poster_image_id, db_media.backdrop_image_id]))
        .find_with_related(ImageSizes)
        .all(db).await?;

    let mut backdrop = Option::None;
    let mut poster = Option::None;
    for (db_image, db_sizes) in images_with_sizes {
        let image = Image {
            path: format!("/{}.{}", db_image.path, db_image.file_type.to_extension()),
            sizes: db_sizes.iter().map(|s| s.into()).collect(),
        };

        match db_image.r#type {
            ImageType::Backdrop => { backdrop = Some(image) }
            ImageType::Poster => { poster = Some(image) }
        }
    }

    Ok((backdrop, poster))
}

pub(crate) async fn delete_unused_image(id: i32, db: &DbConn) -> Result<(), SrvErr> {
    let usages = Media::find()
        .filter(media::Column::BackdropImageId.eq(id).or(media::Column::PosterImageId.eq(id)))
        .count(db).await?;

    let manual_references = ManualImageReferences::find()
        .filter(manual_image_references::Column::ImageId.eq(id))
        .count(db).await?;

    if usages > 0 || manual_references > 0 {
        return Ok(())
    }

    let image = Images::find_by_id(id).one(db).await?;
    if image.is_none() {
        return Ok(())
    }
    let image = image.unwrap();
    let directory_path = Path::new(&config().images.store_path).join(&image.path);
    ImageSizes::delete_many().filter(image_sizes::Column::ImageId.eq(id)).exec(db).await?;
    image.delete(db).await?;

    fs::remove_dir_all(directory_path).await.map_err(|e| {
        error!("Failed to delete directory: {:?}", e);
        SrvErr::Internal("Failed to delete image directory".to_string())
    })
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

async fn save_image_size<C>(image_id: i32, width: u32, height: u32, directory_path: &PathBuf,
                         extension: &str, format: ImageFormat, image: &DynamicImage, db: &C) -> Result<(), SrvErr>  where C : ConnectionTrait {

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
            SrvErr::Internal("Failed to write image".to_string())
        })?;

        let mut file = File::create_new(&path).await.map_err(|e|
            {
                error!("File {} for size {}x{} already exists. Error: {}", path.display(), width, height, e);
                e
            })?;
        if let Err(err) = file.write_all(buffer.as_slice()).await {
            drop(file);
            let _ = fs::remove_file(path).await;
            return Err(SrvErr::from(err));
        }
        file.flush().await?;
    }

    image_sizes::ActiveModel {
        id: Default::default(),
        image_id: Set(image_id),
        width: Set(width as i32),
        height: Set(height as i32),
    }.insert(db).await?;

    Ok(())
}