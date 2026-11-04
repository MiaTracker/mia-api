use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use bytes::Bytes;
use fancy_regex::Regex;
use futures::{Stream, TryStreamExt};
use futures::stream::BoxStream;
use http::StatusCode;
use image::{DynamicImage, ImageFormat, ImageReader};
use image::imageops::FilterType;
use log::{debug, error, warn};
use once_cell::sync::Lazy;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbConn, EntityTrait, ModelTrait, NotSet, Set};
use entities::{episodes, image_sizes, images, manual_image_references, media, seasons};
use entities::prelude::{Episodes, ImageSizes, Images, ManualImageReferences, Media, Seasons};
use entities::sea_orm_active_enums::{ImageFileType, ImageSource, ImageType};
use views::images::Image;
use crate::infrastructure::{ImageStore, Random, SrvErr, Tmdb};
use crate::jobs::{Job, Jobs};
use sea_orm::QueryFilter;
use sea_orm::PaginatorTrait;

static SLUG_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^\\d+x\\d+$").expect("Invalid slug regex")
});

static DIRECTORY_NAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("^[0-9abcdef]{32}$").expect("Invalid directory name regex")
});

pub async fn get_local(slug: String, name: String, store: &dyn ImageStore) -> Result<(&'static str, BoxStream<'static, std::io::Result<Bytes>>), SrvErr> {
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

    let directory = directory_name.to_str().ok_or(SrvErr::BadRequest("File name invalid".to_string()))?;
    let extension = extension.to_str().ok_or(SrvErr::BadRequest("File name invalid".to_string()))?;
    let stream = store.open(directory, &format!("{slug}.{extension}")).await?;

    Ok((content_type, stream))
}

/// The returned stream does not borrow `tmdb` (`use<>`), so it can outlive the request handler.
pub async fn get_tmdb(slug: String, path: String, tmdb: &dyn Tmdb) -> Result<(StatusCode, String, impl Stream<Item=Result<Bytes, std::io::Error>> + use<>), SrvErr> {
    let path = format!("/{}", path);
    let (status_code, content_type, stream) = tmdb.image_stream(slug, path).await?;
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

/// Saves the original size of a TMDB image (and its `images` row) now, and submits a
/// `Job::SaveImageSize` for each smaller size, smallest first. Returns the image's id; if an image
/// with this source path exists already, returns its id and does nothing else.
pub async fn save_tmdb_image(tmdb_path: &str, r#type: ImageType, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, random: &dyn Random) -> Result<i32, SrvErr> {
    let existing = Images::find().filter(images::Column::SourcePath.eq(tmdb_path)).one(db).await?;

    if let Some(existing_image) = existing {
        return Ok(existing_image.id);
    }

    let backdrop_bytes = tmdb.image(tmdb_path).await?;

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

    let extension: &'static str = image_file_type.to_extension();

    let directory = loop {
        let uuid = random.uuid().simple().to_string();

        let conflicts = Images::find().filter(images::Column::Path.eq(&uuid))
            .count(db).await?;

        if conflicts == 0 {
            break uuid;
        }
    };

    let width = image.width();
    let height = image.height();

    let db_image = images::ActiveModel {
        id: NotSet,
        path: Set(directory.clone()),
        source: Set(ImageSource::Tmdb),
        source_path: Set(Some(tmdb_path.to_string())),
        r#type: Set(r#type),
        file_type: Set(image_file_type.clone())
    }.insert(db).await?;

    save_image_size(db_image.id, width, height, &directory, extension, format, &image, db, store).await?;

    let image = Arc::new(image);
    for (resized_width, resized_height) in remaining_sizes(width, height) {
        jobs.submit(Job::SaveImageSize {
            image_id: db_image.id,
            width: resized_width,
            height: resized_height,
            directory: directory.clone(),
            extension,
            format,
            image: image.clone(),
        }).await;
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
            ImageType::Still => {}
        }
    }

    Ok((backdrop, poster))
}

pub(crate) async fn delete_unused_image(id: i32, db: &impl ConnectionTrait, store: &dyn ImageStore) -> Result<(), SrvErr> {
    if let Some(directory) = delete_unused_image_rows(id, db).await? {
        store.delete_directory(&directory).await?;
    }
    Ok(())
}

/// The database half of `delete_unused_image`: when nothing uses the image, deletes its rows and
/// returns the directory that holds its files. The caller deletes that directory, so that a
/// caller working inside a transaction can do it after the commit: files can't be rolled back.
pub(crate) async fn delete_unused_image_rows(id: i32, db: &impl ConnectionTrait) -> Result<Option<String>, SrvErr> {
    let usages = Media::find()
        .filter(media::Column::BackdropImageId.eq(id).or(media::Column::PosterImageId.eq(id)))
        .count(db).await?;

    let manual_references = ManualImageReferences::find()
        .filter(manual_image_references::Column::ImageId.eq(id))
        .count(db).await?;

    let season_usages = Seasons::find()
        .filter(seasons::Column::PosterImageId.eq(id))
        .count(db).await?;

    let episode_usages = Episodes::find()
        .filter(episodes::Column::StillImageId.eq(id))
        .count(db).await?;

    if usages > 0 || manual_references > 0 || season_usages > 0 || episode_usages > 0 {
        return Ok(None)
    }

    let image = Images::find_by_id(id).one(db).await?;
    if image.is_none() {
        return Ok(None)
    }
    let image = image.unwrap();
    let directory = image.path.clone();
    ImageSizes::delete_many().filter(image_sizes::Column::ImageId.eq(id)).exec(db).await?;
    image.delete(db).await?;

    Ok(Some(directory))
}

/// The sizes to save besides the original: for each threshold `100 + 200 * i` up to
/// `short side - 200`, the smallest multiple of the aspect-ratio step that reaches it (see
/// `calculate_image_size`). Smallest first. None when the short side is under 200 px.
fn remaining_sizes(width: u32, height: u32) -> Vec<(u32, u32)> {
    let gcd = gcd::binary_u32(width, height);

    let width_short = width <= height;
    let short = if width_short { width } else { height };
    let long = if width_short { height } else { width };
    let step = short / gcd;

    let ratio = long as f32 / short as f32;

    let mut sizes = Vec::new();
    let Some(limit) = short.checked_sub(200) else {
        return sizes;
    };
    let mut i = 0;
    loop {
        let threshold = 100 + 200 * i;
        if threshold > limit {
            break;
        }
        sizes.push(calculate_image_size(threshold, width_short, step, ratio));
        i += 1;
    }
    sizes
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

/// The `Job::SaveImageSize` handler: resizes `image` to `width` x `height` (unless it already has
/// that size), writes it as `{width}x{height}.{extension}` and inserts the `image_sizes` row.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_image_size(image_id: i32, width: u32, height: u32, directory: &str,
                         extension: &str, format: ImageFormat, image: &DynamicImage, db: &DbConn,
                         store: &dyn ImageStore) -> Result<(), SrvErr> {

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

        store.write_new(directory, &format!("{}x{}.{}", width, height, extension), buffer).await?;
    }

    image_sizes::ActiveModel {
        id: Default::default(),
        image_id: Set(image_id),
        width: Set(width as i32),
        height: Set(height as i32),
    }.insert(db).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use sea_orm::Value;
    use crate::test_support;
    use crate::jobs::{self, Job};
    use crate::test_support::{InMemoryImageStore, MockTmdb, RecordingJobs, SeqRandom};
    use uuid::Uuid;
    use crate::test_support::jobs::{job_deps, sizes};
    use crate::test_support::tmdb::png_bytes;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote};
    use crate::test_support::fixtures;

    #[tokio::test]
    async fn get_tmdb_passes_through_status_and_content_type() {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image_stream()
            .withf(|slug, path| slug == "w500" && path == "/poster.jpg")
            .times(1)
            .returning(|_, _| {
                let body = futures::stream::iter([Ok(Bytes::from_static(b"jpeg-bytes"))]).boxed();
                Ok((StatusCode::OK, "image/jpeg".to_string(), Some(body)))
            });

        let (status, content_type, stream) = get_tmdb("w500".into(), "poster.jpg".into(), &tmdb).await
            .ok().expect("get_tmdb should succeed");

        assert_eq!(status, StatusCode::OK);
        assert_eq!(content_type, "image/jpeg");
        let body: Vec<Bytes> = stream.try_collect().await.expect("stream should yield the body");
        assert_eq!(body.concat(), b"jpeg-bytes");
    }

    #[tokio::test]
    async fn get_tmdb_returns_err_when_tmdb_fails() {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image_stream().times(1).returning(|_, _| Err(test_support::tmdb::error("connection failed")));

        let result = get_tmdb("w500".into(), "poster.jpg".into(), &tmdb).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
    }

    #[tokio::test]
    async fn get_tmdb_returns_not_found_when_tmdb_answers_404() {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image_stream().times(1)
            .returning(|_, _| Ok((StatusCode::NOT_FOUND, "application/json".to_string(), None)));

        let result = get_tmdb("w500".into(), "missing.jpg".into(), &tmdb).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
    }

    const DIRECTORY: &str = "0123456789abcdef0123456789abcdef";

    async fn collect(stream: BoxStream<'static, std::io::Result<Bytes>>) -> Vec<u8> {
        let chunks: Vec<Bytes> = stream.try_collect().await.expect("stream should yield the file");
        chunks.concat()
    }

    fn assert_bad_request<T>(result: Result<T, SrvErr>) {
        assert!(matches!(result, Err(SrvErr::BadRequest(_))), "expected a bad request");
    }

    #[tokio::test]
    async fn get_local_returns_bad_request_when_name_not_32_hex() {
        let store = InMemoryImageStore::new();

        assert_bad_request(get_local("300x450".into(), "not-a-directory.jpg".into(), &store).await);
        assert_bad_request(get_local("300x450".into(), "0123456789ABCDEF0123456789ABCDEF.jpg".into(), &store).await);
    }

    #[tokio::test]
    async fn get_local_returns_bad_request_when_slug_not_dimensions() {
        let store = InMemoryImageStore::new();

        assert_bad_request(get_local("../300x450".into(), format!("{DIRECTORY}.jpg"), &store).await);
        assert_bad_request(get_local("300".into(), format!("{DIRECTORY}.jpg"), &store).await);
    }

    #[tokio::test]
    async fn get_local_returns_bad_request_when_extension_unknown() {
        let store = InMemoryImageStore::new();

        assert_bad_request(get_local("300x450".into(), format!("{DIRECTORY}.gif"), &store).await);
        assert_bad_request(get_local("300x450".into(), DIRECTORY.to_string(), &store).await);
    }

    #[tokio::test]
    async fn get_local_returns_not_found_when_file_missing() {
        let store = InMemoryImageStore::new();
        store.insert(DIRECTORY, "300x450.jpg", b"jpeg".to_vec());

        let result = get_local("301x450".into(), format!("{DIRECTORY}.jpg"), &store).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
    }

    #[tokio::test]
    async fn get_local_streams_file_with_mime_type() {
        let store = InMemoryImageStore::new();
        store.insert(DIRECTORY, "300x450.webp", b"webp-bytes".to_vec());

        let (mime, stream) = get_local("300x450".into(), format!("{DIRECTORY}.webp"), &store).await
            .ok().expect("get_local should succeed");

        assert_eq!(mime, "image/webp");
        assert_eq!(collect(stream).await, b"webp-bytes");
    }

    fn store_with_image_files() -> InMemoryImageStore {
        let store = InMemoryImageStore::new();
        store.insert("image-5", "300x450.jpg", b"a".to_vec());
        store.insert("image-5", "100x150.jpg", b"b".to_vec());
        store.insert("other", "300x450.jpg", b"c".to_vec());
        store
    }

    fn counts(counts: [i64; 4]) -> Vec<[std::collections::BTreeMap<&'static str, Value>; 1]> {
        counts.into_iter().map(|n| [fixtures::count_row(n)]).collect()
    }

    #[tokio::test]
    async fn delete_unused_image_keeps_image_when_media_uses_it() {
        let store = store_with_image_files();
        let db = test_support::db::mock().append_query_results(counts([1, 0, 0, 0])).into_connection();

        delete_unused_image(5, &db, &store).await.ok().expect("delete_unused_image should succeed");

        assert_eq!(store.files().len(), 3, "no file should have been removed");
        let log = test_support::db::log(db);
        assert_statement_count(&log, 4);
        assert_no_statement_starting_with(&log, "DELETE");
    }

    #[tokio::test]
    async fn delete_unused_image_keeps_image_when_season_or_episode_uses_it() {
        for usage in [[0, 1, 0, 0], [0, 0, 1, 0], [0, 0, 0, 1]] {
            let store = store_with_image_files();
            let db = test_support::db::mock().append_query_results(counts(usage)).into_connection();

            delete_unused_image(5, &db, &store).await.ok().expect("delete_unused_image should succeed");

            assert_eq!(store.files().len(), 3, "no file should have been removed for usage {usage:?}");
            let log = test_support::db::log(db);
            assert_statement_count(&log, 4);
            assert_no_statement_starting_with(&log, "DELETE");
        }
    }

    #[tokio::test]
    async fn delete_unused_image_removes_rows_and_directory_when_unused() {
        let store = store_with_image_files();
        let db = test_support::db::mock()
            .append_query_results(counts([0, 0, 0, 0]))
            .append_query_results([[fixtures::image_model(5)]])
            .append_exec_results([test_support::db::deleted(), test_support::db::deleted()])
            .into_connection();

        delete_unused_image(5, &db, &store).await.ok().expect("delete_unused_image should succeed");

        assert_eq!(store.files(), vec![("other".to_string(), "300x450.jpg".to_string())]);
        let log = test_support::db::log(db);
        assert_wrote(&log, "DELETE FROM \"image_sizes\"", &[Value::from(5)]);
        assert_wrote(&log, "DELETE FROM \"images\"", &[Value::from(5)]);
    }

    const TMDB_PATH: &str = "/poster.png";

    fn tmdb_returning(bytes: Bytes) -> MockTmdb {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_image()
            .withf(|path| path == TMDB_PATH)
            .times(1)
            .returning(move |_| Ok(bytes.clone()));
        tmdb
    }

    /// A mock DB for a `save_tmdb_image` call that saves a new image: no image with the source
    /// path, no directory conflict, then the `images` and first `image_sizes` inserts.
    fn db_for_new_image(image_id: i32, width: i32, height: i32) -> DbConn {
        test_support::db::mock()
            .append_query_results([Vec::<images::Model>::new()])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[images::Model {
                source_path: Some(TMDB_PATH.into()), file_type: ImageFileType::Png, ..fixtures::image_model(image_id)
            }]])
            .append_query_results([[fixtures::image_size_model(1, image_id, width, height)]])
            .into_connection()
    }

    /// Saves a new `width` x `height` PNG as image 7, returning the store, the recorded jobs and
    /// the mock DB.
    async fn save_new_png(width: u32, height: u32) -> (Result<i32, SrvErr>, InMemoryImageStore, Vec<Job>, DbConn) {
        let tmdb = tmdb_returning(png_bytes(width, height));
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();
        let db = db_for_new_image(7, width as i32, height as i32);

        let result = save_tmdb_image(TMDB_PATH, ImageType::Poster, &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        (result, store, jobs.take(), db)
    }

    #[tokio::test]
    async fn save_tmdb_image_returns_existing_id_when_source_path_known() {
        let tmdb = MockTmdb::new();
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();
        let db = test_support::db::mock().append_query_results([[fixtures::image_model(5)]]).into_connection();

        let id = save_tmdb_image(TMDB_PATH, ImageType::Poster, &db, &tmdb, &store, &jobs, &SeqRandom::new()).await
            .ok().expect("save_tmdb_image should succeed");

        assert_eq!(id, 5);
        assert!(jobs.take().is_empty(), "no jobs should be submitted");
        assert!(store.files().is_empty(), "no file should be written");
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn save_tmdb_image_saves_original_and_submits_remaining_sizes() {
        let (result, store, jobs, db) = save_new_png(1000, 1500).await;

        assert_eq!(result.ok(), Some(7));
        let files = store.files();
        assert_eq!(files.len(), 1, "only the original should be written: {files:?}");
        let (directory, file_name) = &files[0];
        assert_eq!(file_name, "1000x1500.png");
        assert_eq!(directory, &Uuid::from_u128(1000).simple().to_string());
        let log = test_support::db::log(db);
        assert_statement_count(&log, 4);
        assert_wrote(&log, "INSERT INTO \"images\"", &[Value::from(TMDB_PATH), Value::from(directory.as_str())]);
        assert_wrote(&log, "INSERT INTO \"image_sizes\"", &[Value::from(7), Value::from(1000), Value::from(1500)]);
        assert_eq!(sizes(&jobs), vec![(7, 100, 150), (7, 300, 450), (7, 500, 750), (7, 700, 1050)]);
        for job in &jobs {
            let Job::SaveImageSize { directory: job_directory, extension, format, .. } = job else {
                panic!("expected only SaveImageSize jobs, got {job:?}");
            };
            assert_eq!(job_directory, directory);
            assert_eq!(*extension, "png");
            assert_eq!(*format, ImageFormat::Png);
        }
    }

    #[tokio::test]
    async fn save_tmdb_image_submits_one_size_when_short_side_is_300() {
        let (result, _, jobs, _) = save_new_png(300, 450).await;

        assert_eq!(result.ok(), Some(7));
        assert_eq!(sizes(&jobs), vec![(7, 100, 150)]);
    }

    #[tokio::test]
    async fn save_tmdb_image_submits_no_jobs_when_short_side_under_300() {
        let (result, store, jobs, _) = save_new_png(299, 450).await;

        assert_eq!(result.ok(), Some(7));
        assert_eq!(store.files().len(), 1, "the original should still be written");
        assert_eq!(sizes(&jobs), vec![]);
    }

    #[tokio::test]
    async fn save_tmdb_image_does_not_panic_on_tiny_image() {
        let (result, store, jobs, _) = save_new_png(150, 150).await;

        assert_eq!(result.ok(), Some(7));
        assert_eq!(store.files().len(), 1, "the original should still be written");
        assert_eq!(sizes(&jobs), vec![]);
    }

    #[tokio::test]
    async fn save_tmdb_image_returns_integration_error_when_bytes_not_an_image() {
        let tmdb = tmdb_returning(Bytes::from_static(b"definitely not an image"));
        let store = InMemoryImageStore::new();
        let jobs = RecordingJobs::new();
        let db = test_support::db::mock().append_query_results([Vec::<images::Model>::new()]).into_connection();

        let result = save_tmdb_image(TMDB_PATH, ImageType::Poster, &db, &tmdb, &store, &jobs, &SeqRandom::new()).await;

        assert!(matches!(result, Err(SrvErr::Integration(_))));
        assert!(jobs.take().is_empty(), "no jobs should be submitted");
        assert!(store.files().is_empty(), "no file should be written");
        assert_no_statement_starting_with(&test_support::db::log(db), "INSERT");
    }

    #[tokio::test]
    async fn run_save_image_size_writes_resized_file_and_row() {
        let (_, store, jobs, _) = save_new_png(1000, 1500).await;
        let (directory, _) = store.files()[0].clone();
        let db = Arc::new(test_support::db::mock()
            .append_query_results((1..=4).map(|id| [fixtures::image_size_model(id, 7, 0, 0)]))
            .into_connection());
        let deps = job_deps(db.clone(), Arc::new(store.clone()));

        for job in jobs {
            jobs::run(job, &deps).await.ok().expect("the job should succeed");
        }

        let expected = [(100, 150), (300, 450), (500, 750), (700, 1050)];
        let mut expected_files: Vec<_> = expected.iter().map(|(w, h)| (directory.clone(), format!("{w}x{h}.png")))
            .chain([(directory.clone(), "1000x1500.png".to_string())])
            .collect();
        expected_files.sort();
        assert_eq!(store.files(), expected_files);
        for (width, height) in expected {
            let bytes = store.get(&directory, &format!("{width}x{height}.png")).expect("the file should exist");
            let decoded = image::load_from_memory(&bytes).expect("the file should be an image");
            assert_eq!((decoded.width(), decoded.height()), (width, height));
        }
        drop(deps);
        let db = Arc::into_inner(db).expect("no other handle to the mock DB");
        let log = test_support::db::log(db);
        assert_statement_count(&log, 4);
        for (width, height) in expected {
            assert_wrote(&log, "INSERT INTO \"image_sizes\"", &[Value::from(7), Value::from(width as i32), Value::from(height as i32)]);
        }
    }

    #[tokio::test]
    async fn run_save_image_size_returns_err_when_file_exists() {
        let store = InMemoryImageStore::new();
        store.insert(DIRECTORY, "100x150.png", b"existing".to_vec());
        let db = Arc::new(test_support::db::mock().into_connection());
        let deps = job_deps(db.clone(), Arc::new(store.clone()));
        let job = Job::SaveImageSize {
            image_id: 7, width: 100, height: 150, directory: DIRECTORY.to_string(), extension: "png",
            format: ImageFormat::Png, image: Arc::new(DynamicImage::new_rgb8(200, 300)),
        };

        let result = jobs::run(job, &deps).await;

        assert!(result.is_err(), "writing over an existing file should fail");
        assert_eq!(store.get(DIRECTORY, "100x150.png"), Some(b"existing".to_vec()));
        drop(deps);
        let db = Arc::into_inner(db).expect("no other handle to the mock DB");
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn fetch_backdrop_and_poster_assigns_by_image_type() {
        let poster = images::Model {
            path: "poster-dir".into(), r#type: ImageType::Poster, file_type: ImageFileType::Jpeg, ..fixtures::image_model(1)
        };
        let backdrop = images::Model {
            path: "backdrop-dir".into(), r#type: ImageType::Backdrop, file_type: ImageFileType::WebP, ..fixtures::image_model(2)
        };
        let db = test_support::db::mock()
            .append_query_results([vec![
                (poster, Some(fixtures::image_size_model(1, 1, 300, 450))),
                (backdrop, Some(fixtures::image_size_model(2, 2, 1280, 720))),
            ]])
            .into_connection();
        let media = media::Model {
            poster_image_id: Some(1), backdrop_image_id: Some(2), ..fixtures::movie_media_model(1, 1)
        };

        let (backdrop, poster) = fetch_backdrop_and_poster(&media, &db).await.ok().expect("fetch should succeed");

        let backdrop = backdrop.expect("backdrop should be set");
        let poster = poster.expect("poster should be set");
        assert_eq!(backdrop.path, "/backdrop-dir.webp");
        assert_eq!(backdrop.sizes.len(), 1);
        assert_eq!(poster.path, "/poster-dir.jpg");
        assert_eq!(poster.sizes.len(), 1);
    }
}