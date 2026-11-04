//! Functions that write several rows must leave nothing behind when a later write fails. Each test
//! installs a trigger that makes one of the function's later writes fail, then checks that its
//! earlier write was rolled back. A mock database can't see transaction-scope mistakes (it logs
//! every statement after `BEGIN` as inside the transaction), so these run against Postgres. Needs Docker:
//! `cargo test -p services --test postgres_transactions -- --ignored`.

mod common;

use common::postgres::database;
use common::seed;
use common::seed::fail_on;
use entities::prelude::{Genres, Languages, Logs, Media, MediaGenres, MediaTags, Sources, Tags, Titles};
use entities::sea_orm_active_enums::MediaType;
use entities::{media, titles};
use integrations::tmdb::views::Languages as TmdbLanguage;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter, QueryOrder};
use services::test_support::tmdb::genre_list;
use services::test_support::MockTmdb;
use services::{genres, logs, masterdata, media as media_service, movies, series, sources, tags, titles as titles_service};
use views::genres::GenreCreate;
use views::logs::{LogCreate, LogUpdate};
use views::media::MediaType as ViewMediaType;
use views::movies::MovieMetadata;
use views::series::SeriesMetadata;
use views::sources::{SourceCreate, SourceType, SourceUpdate};
use views::tags::TagCreate;
use views::titles::TitleCreate;
use views::users::CurrentUser;

const MOVIE: ViewMediaType = ViewMediaType::Movie;

/// A user and their movie "Movie" (TMDB id 1, primary title only).
async fn movie(db: &DbConn) -> (CurrentUser, media::Model) {
    let user = seed::user(db, 1).await;
    let movie = seed::movie(db, &user, 1, "Movie").await;
    (user, movie)
}

/// A user and their movie that is bot-controllable, so a change by the (non-bot) user also
/// updates the media row after its first write.
async fn bot_controllable_movie(db: &DbConn) -> (CurrentUser, media::Model) {
    let (user, movie) = movie(db).await;
    seed::bot_controllable(db, movie.id).await;
    (user, movie)
}

async fn titles_of(db: &DbConn, media_id: i32) -> Vec<(String, bool)> {
    Titles::find().filter(titles::Column::MediaId.eq(media_id)).order_by_asc(titles::Column::Id).all(db).await
        .expect("querying titles").into_iter().map(|t| (t.title, t.primary)).collect()
}

async fn overview(db: &DbConn, media_id: i32) -> Option<String> {
    Media::find_by_id(media_id).one(db).await.expect("querying media").expect("media row").overview
}

#[tokio::test]
#[ignore = "requires docker"]
async fn genres_create_rolls_back_new_genre_when_linking_it_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    fail_on(&db, "media_genres", "INSERT").await;

    let result = genres::create(movie.id, &GenreCreate { name: "Heist".into() }, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Genres).await, 0, "the new genre is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn genres_delete_keeps_genre_link_when_deleting_unused_genre_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    let genre = seed::genre(&db, movie.id, "Heist", MediaType::Movie, None).await;
    fail_on(&db, "genres", "DELETE").await;

    let result = genres::delete(movie.id, genre.id, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, MediaGenres).await, 1, "the removed link is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn tags_create_rolls_back_new_tag_when_linking_it_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    fail_on(&db, "media_tags", "INSERT").await;

    let result = tags::create(movie.id, TagCreate { name: "Favourite".into() }, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Tags).await, 0, "the new tag is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn tags_delete_keeps_tag_link_when_deleting_unused_tag_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    let tag = seed::tag(&db, movie.id, "Favourite").await;
    fail_on(&db, "tags", "DELETE").await;

    let result = tags::delete(movie.id, tag.id, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, MediaTags).await, 1, "the removed link is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn titles_create_rolls_back_title_when_updating_media_fails() {
    let db = database().await;
    let (user, movie) = bot_controllable_movie(&db).await;
    fail_on(&db, "media", "UPDATE").await;

    let result = titles_service::create(movie.id, &TitleCreate { name: "Alternative".into() }, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(titles_of(&db, movie.id).await, vec![("Movie".to_string(), true)], "the new title is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn titles_set_primary_keeps_primary_title_when_updating_media_fails() {
    let db = database().await;
    let (user, movie) = bot_controllable_movie(&db).await;
    let alternative = seed::title(&db, movie.id, "Alternative", false).await;
    fail_on(&db, "media", "UPDATE").await;

    let result = titles_service::set_primary(movie.id, alternative.id, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(titles_of(&db, movie.id).await, vec![("Movie".to_string(), true), ("Alternative".to_string(), false)],
               "the primary flags are rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn titles_delete_keeps_primary_title_when_promoting_next_title_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    seed::title(&db, movie.id, "Alternative", false).await;
    let primary = Titles::find().filter(titles::Column::Primary.eq(true)).one(&db).await.expect("querying titles").expect("primary title");
    fail_on(&db, "titles", "UPDATE").await;

    let result = titles_service::delete(movie.id, primary.id, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(titles_of(&db, movie.id).await, vec![("Movie".to_string(), true), ("Alternative".to_string(), false)],
               "the deleted title is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sources_create_rolls_back_source_when_updating_media_fails() {
    let db = database().await;
    let (user, movie) = bot_controllable_movie(&db).await;
    fail_on(&db, "media", "UPDATE").await;
    let create = SourceCreate { name: "Jellyfin".into(), url: "https://jellyfin.example".into(), r#type: SourceType::Jellyfin };

    let result = sources::create(movie.id, &create, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Sources).await, 0, "the new source is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sources_update_keeps_source_when_updating_media_fails() {
    let db = database().await;
    let (user, movie) = bot_controllable_movie(&db).await;
    let source = seed::source(&db, movie.id, "Jellyfin").await;
    fail_on(&db, "media", "UPDATE").await;
    let update = SourceUpdate { id: source.id, name: "Web".into(), url: "https://web.example".into(), r#type: SourceType::Web };

    let result = sources::update(movie.id, source.id, &update, MOVIE, &user, &db).await;

    assert!(result.is_err());
    let kept = Sources::find_by_id(source.id).one(&db).await.expect("querying sources").expect("the source");
    assert_eq!(kept.name, "Jellyfin", "the source's update is rolled back");
}

/// `sources::delete` deletes through `sources::delete_from_media`.
#[tokio::test]
#[ignore = "requires docker"]
async fn sources_delete_keeps_source_when_updating_media_fails() {
    let db = database().await;
    let (user, movie) = bot_controllable_movie(&db).await;
    let source = seed::source(&db, movie.id, "Jellyfin").await;
    fail_on(&db, "media", "UPDATE").await;

    let result = sources::delete(movie.id, source.id, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Sources).await, 1, "the deleted source is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn logs_create_rolls_back_log_when_updating_media_rating_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    seed::source(&db, movie.id, "Jellyfin").await;
    fail_on(&db, "media", "UPDATE").await;
    let create = LogCreate { date: seed::date_added(), stars: Some(7.0), completed: true, comment: None, source: "Jellyfin".into() };

    let result = logs::create(movie.id, &create, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Logs).await, 0, "the new log is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn logs_update_keeps_log_when_updating_media_rating_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    let source = seed::source(&db, movie.id, "Jellyfin").await;
    let log = seed::log(&db, movie.id, source.id, Some(5.0), true).await;
    fail_on(&db, "media", "UPDATE").await;
    let update = LogUpdate { id: log.id, date: seed::date_added(), stars: Some(8.0), completed: true, comment: None, source: "Jellyfin".into() };

    let result = logs::update(movie.id, log.id, &update, MOVIE, &user, &db).await;

    assert!(result.is_err());
    let kept = Logs::find_by_id(log.id).one(&db).await.expect("querying logs").expect("the log");
    assert_eq!(kept.stars, Some(5.0), "the log's update is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn logs_delete_keeps_log_when_updating_media_rating_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    let source = seed::source(&db, movie.id, "Jellyfin").await;
    let log = seed::log(&db, movie.id, source.id, Some(5.0), true).await;
    fail_on(&db, "media", "UPDATE").await;

    let result = logs::delete(movie.id, log.id, MOVIE, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Logs).await, 1, "the deleted log is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn movies_update_keeps_media_when_updating_movie_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    fail_on(&db, "movies", "UPDATE").await;
    let metadata = MovieMetadata {
        id: movie.id, homepage: None, imdb_id: None, title: Some("Movie".into()), overview: Some("Changed".into()),
        original_language: None, origin_country: None, release_date: None, runtime: Some(120), status: None,
    };

    let result = movies::update(movie.id, metadata, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(overview(&db, movie.id).await, movie.overview, "the media update is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn series_update_keeps_media_when_updating_series_fails() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let show = seed::series(&db, &user, 1, "Series").await;
    fail_on(&db, "series", "UPDATE").await;
    let metadata = SeriesMetadata {
        id: show.id, homepage: None, imdb_id: None, title: Some("Series".into()), overview: Some("Changed".into()),
        original_language: None, origin_country: None, first_air_date: None, status: Some("Ended".into()), r#type: None,
    };

    let result = series::update(show.id, metadata, &user, &db).await;

    assert!(result.is_err());
    assert_eq!(overview(&db, show.id).await, show.overview, "the media update is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn masterdata_refresh_rolls_back_languages_when_saving_genres_fails() {
    let db = database().await;
    fail_on(&db, "genres", "INSERT").await;
    let mut tmdb = MockTmdb::new();
    tmdb.expect_languages().returning(|| Ok(vec![
        TmdbLanguage { iso_639_1: "en".into(), english_name: "English".into(), name: "English".into() },
    ]));
    tmdb.expect_movie_genres().returning(|| Ok(genre_list(&[(28, "Action")])));
    tmdb.expect_tv_genres().returning(|| Ok(genre_list(&[(18, "Drama")])));

    let result = masterdata::refresh(&db, &tmdb).await;

    assert!(result.is_err());
    assert_eq!(seed::count(&db, Languages).await, 0, "the new language is rolled back");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn media_delete_keeps_media_when_deleting_orphaned_tags_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    let store = services::test_support::InMemoryImageStore::new();
    fail_on(&db, "tags", "DELETE").await;

    let result = media_service::delete(movie.id, MOVIE, &user, &db, &store).await;

    assert!(result.is_err());
    assert!(Media::find_by_id(movie.id).one(&db).await.expect("querying media").is_some(), "the media deletion is rolled back");
    assert_eq!(titles_of(&db, movie.id).await, vec![("Movie".to_string(), true)]);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn media_delete_w_source_keeps_media_when_deleting_orphaned_tags_fails() {
    let db = database().await;
    let (user, movie) = movie(&db).await;
    seed::source(&db, movie.id, "Jellyfin").await;
    fail_on(&db, "tags", "DELETE").await;

    let result = media_service::delete_w_source(1, "Jellyfin".into(), MOVIE, &user, &db, &services::test_support::InMemoryImageStore::new()).await;

    assert!(result.is_err());
    assert!(Media::find_by_id(movie.id).one(&db).await.expect("querying media").is_some(), "the media deletion is rolled back");
}
