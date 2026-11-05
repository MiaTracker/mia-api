//! Rows inserted straight into a test database, and checks on what's in it.

use chrono::NaiveDate;
use chrono::{DateTime, FixedOffset, TimeZone, Utc};
use entities::sea_orm_active_enums::{ExchangeStatus, ImageFileType, ImageSource, ImageType, LlmCallPurpose, MediaType, MessageRole, SourceType};
use entities::{conversations, episodes, exchanges, genres, images, languages, llm_calls, logs, manual_image_references, media, media_genres, media_tags, messages, movies, seasons, series, sources, tags, titles, users, watchlist};
use sea_orm::{ActiveModelTrait, ConnectionTrait, DbConn, EntityTrait, NotSet, PaginatorTrait, Set};
use serde_json::json;
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

/// A log on `date` with `comment`, otherwise like `log`.
pub async fn log_on(db: &DbConn, media_id: i32, source_id: i32, date: NaiveDate, comment: Option<&str>) -> logs::Model {
    logs::ActiveModel {
        id: NotSet,
        media_id: Set(media_id),
        date: Set(date),
        stars: Set(None),
        completed: Set(true),
        comment: Set(comment.map(str::to_string)),
        source_id: Set(source_id),
        original_stars: Set(None),
    }.insert(db).await.expect("inserting log")
}

pub async fn tag(db: &DbConn, media_id: i32, name: &str) -> tags::Model {
    let tag = tags::ActiveModel { id: NotSet, name: Set(name.into()) }.insert(db).await.expect("inserting tag");
    media_tags::ActiveModel { media_id: Set(media_id), tag_id: Set(tag.id) }.insert(db).await.expect("inserting media tag");
    tag
}

/// Puts an existing tag on another media.
pub async fn add_tag(db: &DbConn, media_id: i32, tag: &tags::Model) {
    media_tags::ActiveModel { media_id: Set(media_id), tag_id: Set(tag.id) }.insert(db).await.expect("inserting media tag");
}

/// A genre of `media_type` on the media; `tmdb_id` is `None` for a user-defined genre.
pub async fn genre(db: &DbConn, media_id: i32, name: &str, media_type: MediaType, tmdb_id: Option<i32>) -> genres::Model {
    let genre = genres::ActiveModel { id: NotSet, tmdb_id: Set(tmdb_id), name: Set(name.into()), r#type: Set(media_type) }
        .insert(db).await.expect("inserting genre");
    media_genres::ActiveModel { media_id: Set(media_id), genre_id: Set(genre.id) }.insert(db).await.expect("inserting media genre");
    genre
}

/// A manual poster (PNG, directory `{n:032x}`, no size rows) referenced by the media.
pub async fn manual_image(db: &DbConn, media_id: i32, n: u128) -> images::Model {
    let image = images::ActiveModel {
        id: NotSet,
        path: Set(format!("{n:032x}")),
        source: Set(ImageSource::Manual),
        source_path: Set(None),
        r#type: Set(ImageType::Poster),
        file_type: Set(ImageFileType::Png),
    }.insert(db).await.expect("inserting image");
    manual_image_references::ActiveModel { image_id: Set(image.id), media_id: Set(media_id) }
        .insert(db).await.expect("inserting manual image reference");
    image
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

/// A season numbered `number` of the series, without episodes or images.
pub async fn season(db: &DbConn, series_id: i32, number: i32) -> seasons::Model {
    seasons::ActiveModel {
        id: NotSet,
        series_id: Set(series_id),
        air_date: Set(None),
        episode_count: Set(None),
        name: Set(Some(format!("Season {number}"))),
        overview: Set(None),
        season_number: Set(Some(number)),
        tmdb_vote_average: Set(None),
        stars: Set(None),
        tmdb_id: Set(None),
        poster_image_id: Set(None),
    }.insert(db).await.expect("inserting season")
}

/// An episode numbered `number` of the season, without a still.
pub async fn episode(db: &DbConn, season_id: i32, number: i32) -> episodes::Model {
    episodes::ActiveModel {
        id: NotSet,
        season_id: Set(season_id),
        tmdb_id: Set(Some(number)),
        episode_number: Set(Some(number)),
        name: Set(Some(format!("Episode {number}"))),
        overview: Set(None),
        air_date: Set(None),
        runtime: Set(None),
        tmdb_vote_average: Set(None),
        still_image_id: Set(None),
    }.insert(db).await.expect("inserting episode")
}

/// When seeded chat rows were created: 2024-01-01 12:00 UTC.
pub fn chat_time() -> DateTime<FixedOffset> {
    Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).single().expect("valid time").fixed_offset()
}

/// An untitled Conversation of the user.
pub async fn conversation(db: &DbConn, user: &CurrentUser) -> conversations::Model {
    conversations::ActiveModel {
        id: NotSet,
        user_id: Set(user.id),
        title: Set(None),
        created_at: Set(chat_time()),
        updated_at: Set(chat_time()),
    }.insert(db).await.expect("inserting conversation")
}

/// An Exchange of the Conversation with `status`; one that is not in progress is also completed.
pub async fn exchange(db: &DbConn, conversation_id: i32, status: ExchangeStatus) -> exchanges::Model {
    let completed_at = (status != ExchangeStatus::InProgress).then(chat_time);
    exchanges::ActiveModel {
        id: NotSet,
        conversation_id: Set(conversation_id),
        status: Set(status),
        error: Set(None),
        created_at: Set(chat_time()),
        completed_at: Set(completed_at),
    }.insert(db).await.expect("inserting exchange")
}

/// A user Message of the Exchange holding one text block.
pub async fn user_message(db: &DbConn, exchange_id: i32, text: &str) -> messages::Model {
    messages::ActiveModel {
        id: NotSet,
        exchange_id: Set(exchange_id),
        role: Set(MessageRole::User),
        content: Set(json!([{"type": "text", "text": text}])),
        llm_call_id: Set(None),
        created_at: Set(chat_time()),
    }.insert(db).await.expect("inserting message")
}

/// A Message of the Exchange with `role` and Anthropic `content` blocks. An assistant Message needs
/// the `llm_call_id` of the call that produced it; the others must have none.
pub async fn message(db: &DbConn, exchange_id: i32, role: MessageRole, content: serde_json::Value, llm_call_id: Option<i32>) -> messages::Model {
    messages::ActiveModel {
        id: NotSet,
        exchange_id: Set(exchange_id),
        role: Set(role),
        content: Set(content),
        llm_call_id: Set(llm_call_id),
        created_at: Set(chat_time()),
    }.insert(db).await.expect("inserting message")
}

/// An LLM call of the Exchange that has not completed.
pub async fn llm_call(db: &DbConn, user: &CurrentUser, exchange_id: i32) -> llm_calls::Model {
    let exchange = exchanges::Entity::find_by_id(exchange_id).one(db).await.expect("querying exchange").expect("exchange row");
    llm_calls::ActiveModel {
        id: NotSet,
        user_id: Set(Some(user.id)),
        conversation_id: Set(Some(exchange.conversation_id)),
        exchange_id: Set(Some(exchange_id)),
        purpose: Set(LlmCallPurpose::Exchange),
        model: Set("test/model".into()),
        stop_reason: Set(None),
        input_tokens: Set(None),
        output_tokens: Set(None),
        cache_read_tokens: Set(None),
        cache_creation_tokens: Set(None),
        provider_request_id: Set(None),
        error: Set(None),
        created_at: Set(chat_time()),
        completed_at: Set(None),
    }.insert(db).await.expect("inserting LLM call")
}

/// A title call for the Conversation (no Exchange), open unless `completed_at` is given.
pub async fn title_call(db: &DbConn, user: &CurrentUser, conversation_id: i32, completed_at: Option<DateTime<FixedOffset>>) -> llm_calls::Model {
    llm_calls::ActiveModel {
        id: NotSet,
        user_id: Set(Some(user.id)),
        conversation_id: Set(Some(conversation_id)),
        exchange_id: Set(None),
        purpose: Set(LlmCallPurpose::Title),
        model: Set("test/title-model".into()),
        stop_reason: Set(completed_at.map(|_| "end_turn".into())),
        input_tokens: Set(None),
        output_tokens: Set(None),
        cache_read_tokens: Set(None),
        cache_creation_tokens: Set(None),
        provider_request_id: Set(None),
        error: Set(None),
        created_at: Set(chat_time()),
        completed_at: Set(completed_at),
    }.insert(db).await.expect("inserting title call")
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
