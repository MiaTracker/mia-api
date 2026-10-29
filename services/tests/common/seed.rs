//! Rows inserted straight into a test database, and checks on what's in it.

use chrono::NaiveDate;
use entities::sea_orm_active_enums::{MediaType, SourceType};
use entities::{genres, languages, logs, media, media_genres, media_tags, movies, series, sources, tags, titles, users, watchlist};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DbConn, EntityTrait, NotSet, PaginatorTrait, Set};
use uuid::Uuid;
use views::users::CurrentUser;

/// The date seeded media were added on.
pub fn date_added() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid date")
}

/// A non-admin user signed in directly (not through a bot). `n` keeps names and uuids unique.
pub async fn user(db: &DbConn, n: u128) -> CurrentUser {
    let model = users::ActiveModel {
        id: NotSet,
        uuid: Set(Uuid::from_u128(n)),
        email: Set(format!("user{n}@example.com")),
        username: Set(format!("user{n}")),
        password_hash: Set("unused".into()),
        admin: Set(false),
    }.insert(db).await.expect("inserting user");
    CurrentUser::from(model)
}

pub async fn language(db: &DbConn, iso6391: &str, english_name: &str) {
    languages::ActiveModel {
        iso6391: Set(iso6391.into()),
        english_name: Set(english_name.into()),
        name: Set(english_name.into()),
    }.insert(db).await.expect("inserting language");
}

async fn media_row(db: &DbConn, user: &CurrentUser, tmdb_id: i32, media_type: MediaType, title: &str) -> media::Model {
    let media = media::ActiveModel {
        id: NotSet,
        homepage: Set(None),
        tmdb_id: Set(Some(tmdb_id)),
        imdb_id: Set(None),
        overview: Set(Some(format!("Overview of {title}"))),
        tmdb_vote_average: Set(None),
        original_language: Set(None),
        date_added: Set(date_added()),
        r#type: Set(media_type),
        user_id: Set(user.id),
        stars: Set(None),
        bot_controllable: Set(false),
        backdrop_image_id: Set(None),
        poster_image_id: Set(None),
        origin_country: Set(None),
    }.insert(db).await.expect("inserting media");
    self::title(db, media.id, title, true).await;
    media
}

/// A movie with a primary title and nothing else.
pub async fn movie(db: &DbConn, user: &CurrentUser, tmdb_id: i32, title: &str) -> media::Model {
    let media = media_row(db, user, tmdb_id, MediaType::Movie, title).await;
    movies::ActiveModel { id: Set(media.id), release_date: Set(None), runtime: Set(None), status: Set(None) }
        .insert(db).await.expect("inserting movie");
    media
}

/// A series with a primary title and no seasons.
pub async fn series(db: &DbConn, user: &CurrentUser, tmdb_id: i32, title: &str) -> media::Model {
    let media = media_row(db, user, tmdb_id, MediaType::Series, title).await;
    series::ActiveModel { id: Set(media.id), first_air_date: Set(None), status: Set(None), r#type: Set(None) }
        .insert(db).await.expect("inserting series");
    media
}

pub async fn title(db: &DbConn, media_id: i32, title: &str, primary: bool) -> titles::Model {
    titles::ActiveModel { id: NotSet, media_id: Set(media_id), primary: Set(primary), title: Set(title.into()) }
        .insert(db).await.expect("inserting title")
}

/// Changes one media row; set the fields to change on `changes` (its id is filled in).
pub async fn update_media(db: &DbConn, media_id: i32, changes: media::ActiveModel) {
    media::ActiveModel { id: Set(media_id), ..changes }.update(db).await.expect("updating media");
}

/// Marks the media bot-controllable, so a change by a user not acting through a bot also writes
/// the media row (clearing the flag).
pub async fn bot_controllable(db: &DbConn, media_id: i32) {
    update_media(db, media_id, media::ActiveModel { bot_controllable: Set(true), ..Default::default() }).await;
}

pub async fn source(db: &DbConn, media_id: i32, name: &str) -> sources::Model {
    sources::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        name: Set(name.into()),
        url: Set(format!("https://example.com/{name}")),
        r#type: Set(SourceType::Web),
        bot_controllable: Set(false),
    }.insert(db).await.expect("inserting source")
}

pub async fn log(db: &DbConn, media_id: i32, source_id: i32, stars: Option<f32>, completed: bool) -> logs::Model {
    logs::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        date: Set(date_added()),
        stars: Set(stars),
        completed: Set(completed),
        comment: Set(None),
        source_id: Set(source_id),
        original_stars: Set(None),
    }.insert(db).await.expect("inserting log")
}

pub async fn tag(db: &DbConn, media_id: i32, name: &str) -> tags::Model {
    let tag = tags::ActiveModel { id: NotSet, name: Set(name.into()) }.insert(db).await.expect("inserting tag");
    media_tags::ActiveModel { media_id: Set(media_id), tag_id: Set(tag.id) }.insert(db).await.expect("inserting media tag");
    tag
}

/// A genre of `media_type` on the media; `tmdb_id` is `None` for a user-defined genre.
pub async fn genre(db: &DbConn, media_id: i32, name: &str, media_type: MediaType, tmdb_id: Option<i32>) -> genres::Model {
    let genre = genres::ActiveModel { id: NotSet, tmdb_id: Set(tmdb_id), name: Set(name.into()), r#type: Set(media_type) }
        .insert(db).await.expect("inserting genre");
    media_genres::ActiveModel { media_id: Set(media_id), genre_id: Set(genre.id) }.insert(db).await.expect("inserting media genre");
    genre
}

/// A TMDB genre that no media has yet.
pub async fn tmdb_genre(db: &DbConn, tmdb_id: i32, name: &str, media_type: MediaType) -> genres::Model {
    genres::ActiveModel { id: NotSet, tmdb_id: Set(Some(tmdb_id)), name: Set(name.into()), r#type: Set(media_type) }
        .insert(db).await.expect("inserting genre")
}

pub async fn on_watchlist(db: &DbConn, media_id: i32) {
    watchlist::ActiveModel { media_id: Set(media_id), assessment: Set(None), date_added: Set(date_added()) }
        .insert(db).await.expect("inserting watchlist entry");
}

pub async fn count<E: EntityTrait>(db: &DbConn, _entity: E) -> u64 where E::Model: Sync {
    E::find().count(db).await.expect("counting rows")
}

/// Makes every `event` (`INSERT`, `UPDATE` or `DELETE`) statement on `table` fail from now on, so
/// a test can make a function's later write fail and check what its earlier writes left behind.
pub async fn fail_on(db: &DbConn, table: &str, event: &str) {
    db.execute_unprepared(
        "CREATE OR REPLACE FUNCTION mia_test_fail() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN RAISE EXCEPTION 'injected failure: % on %', TG_OP, TG_TABLE_NAME; END $$"
    ).await.expect("creating the failing trigger function");
    db.execute_unprepared(&format!(
        "CREATE TRIGGER mia_test_fail_{table}_{event} BEFORE {event} ON {table} \
         FOR EACH STATEMENT EXECUTE FUNCTION mia_test_fail()",
        event = event.to_lowercase()
    )).await.expect("creating the failing trigger");
}
