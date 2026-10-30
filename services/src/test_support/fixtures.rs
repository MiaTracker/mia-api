//! Builders returning fully populated entity models and views. Tests override fields with
//! struct-update syntax, e.g. `users::Model { admin: true, ..user_model(2) }`.

use std::collections::BTreeMap;
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use sea_orm::{IdenStatic, Iterable, ModelTrait, Value};
use uuid::Uuid;
use entities::sea_orm_active_enums::{ExchangeStatus, ImageFileType, ImageSource, ImageType, LlmCallPurpose, MediaType, MessageRole, SourceType};
use entities::{app_tokens, conversations, episodes, exchanges, genres, image_sizes, images, languages, llm_calls, logs, manual_image_references, media, media_genres, media_locks, media_tags, messages, movie_locks, movies, seasons, series, series_locks, sources, sync_state, tags, titles, users, watchlist};
use views::users::{CurrentUser, UserRegistration};
use super::time::SeqRandom;

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2024, 1, 1).expect("valid date")
}

/// The default authenticated, non-admin user (id 1).
pub fn user() -> CurrentUser {
    CurrentUser::from(user_model(1))
}

/// Like `user()`, but an admin.
pub fn admin() -> CurrentUser {
    CurrentUser { admin: true, ..user() }
}

/// Like `user()`, but authenticated through a bot (an app token).
pub fn bot_user() -> CurrentUser {
    CurrentUser { though_bot: true, ..user() }
}

/// A user row. Id 1 is "tester"; other ids get a derived username and email.
pub fn user_model(id: i32) -> users::Model {
    let (username, email) = if id == 1 {
        ("tester".to_string(), "tester@example.test".to_string())
    } else {
        (format!("tester{id}"), format!("tester{id}@example.test"))
    };
    users::Model {
        id,
        uuid: Uuid::from_u128(id as u128),
        email,
        username,
        password_hash: "unused-password-hash".into(),
        admin: false,
    }
}

pub fn media_model(id: i32, user_id: i32, media_type: MediaType) -> media::Model {
    media::Model {
        id,
        homepage: None,
        tmdb_id: None,
        imdb_id: None,
        overview: None,
        tmdb_vote_average: None,
        original_language: None,
        date_added: date(),
        r#type: media_type,
        user_id,
        stars: None,
        bot_controllable: false,
        backdrop_image_id: None,
        poster_image_id: None,
        origin_country: None,
    }
}

pub fn movie_model(id: i32) -> movies::Model {
    movies::Model {
        id,
        release_date: Some(date()),
        runtime: Some(100),
        status: Some("Released".into()),
    }
}

pub fn series_model(id: i32) -> series::Model {
    series::Model {
        id,
        first_air_date: Some(date()),
        status: Some("Ended".into()),
        r#type: Some("Scripted".into()),
    }
}

pub fn image_model(id: i32) -> images::Model {
    images::Model {
        id,
        path: format!("image-{id}"),
        source: ImageSource::Tmdb,
        source_path: Some(format!("/source-{id}.jpg")),
        r#type: ImageType::Poster,
        file_type: ImageFileType::Jpeg,
    }
}

pub fn image_size_model(id: i32, image_id: i32, width: i32, height: i32) -> image_sizes::Model {
    image_sizes::Model { id, image_id, width, height }
}

pub fn genre_model(id: i32, name: &str, media_type: MediaType) -> genres::Model {
    genres::Model { id, tmdb_id: None, name: name.into(), r#type: media_type }
}

pub fn media_genre_model(media_id: i32, genre_id: i32) -> media_genres::Model {
    media_genres::Model { media_id, genre_id }
}

pub fn language_model(iso6391: &str, english_name: &str) -> languages::Model {
    languages::Model { iso6391: iso6391.into(), english_name: english_name.into(), name: english_name.into() }
}

pub fn movie_media_model(id: i32, user_id: i32) -> media::Model {
    media_model(id, user_id, MediaType::Movie)
}

pub fn tag_model(id: i32, name: &str) -> tags::Model {
    tags::Model { id, name: name.into() }
}

pub fn media_tag_model(media_id: i32, tag_id: i32) -> media_tags::Model {
    media_tags::Model { media_id, tag_id }
}

pub fn title_model(id: i32, media_id: i32, title: &str, primary: bool) -> titles::Model {
    titles::Model { id, media_id, primary, title: title.into() }
}

pub fn source_model(id: i32, media_id: i32, name: &str) -> sources::Model {
    sources::Model {
        id,
        media_id,
        name: name.into(),
        url: format!("https://source-{id}.example.test"),
        r#type: SourceType::Web,
        bot_controllable: false,
    }
}

pub fn log_model(id: i32, media_id: i32, source_id: i32) -> logs::Model {
    logs::Model {
        id,
        media_id,
        date: date(),
        stars: None,
        completed: true,
        comment: None,
        source_id,
        original_stars: None,
    }
}

/// The row a `count()` query returns; queue it with `append_query_results([[count_row(n)]])`.
pub fn count_row(n: i64) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([("num_items", Value::from(n))])
}

/// No rows for `logs::update_media_rating`'s aggregate query (the media has no rated logs).
pub fn no_rating_rows() -> Vec<BTreeMap<&'static str, Value>> {
    Vec::new()
}

/// The row `logs::update_media_rating`'s aggregate query returns: the sum and number of rated logs.
pub fn rating_row(sum: f32, count: i64) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([("sum", Value::from(sum)), ("count", Value::from(count))])
}

/// A `sync_state` row recording a refresh at `synced_at`.
pub fn sync_state_model(id: i32, synced_at: DateTime<Utc>) -> sync_state::Model {
    sync_state::Model { id, synced_at: synced_at.fixed_offset() }
}

/// A row of the `(name, count)` queries behind `statistics::stats` (genres and languages).
pub fn comparative_row(name: &str, count: i64) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([("name", Value::from(name)), ("count", Value::from(count))])
}

/// The row of an `AVG(media.stars)` query; `None` is SQL `NULL` (nothing rated).
pub fn avg_row(avg: Option<f64>) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([("avg", Value::from(avg))])
}

/// The `(id, uuid)` row of a user in `ratings_linearization`'s user query; the uuid is the one `user_model(id)` has.
pub fn user_uuid_row(user_id: i32) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([("id", Value::from(user_id)), ("uuid", Value::from(Uuid::from_u128(user_id as u128)))])
}

/// A row of `ratings_linearization`'s rated-logs query: the log's id, media and its input stars
/// (`COALESCE(original_stars, stars)`).
pub fn rated_log_row(id: i32, media_id: i32, stars: f32) -> BTreeMap<&'static str, Value> {
    BTreeMap::from([("id", Value::from(id)), ("media_id", Value::from(media_id)), ("stars", Value::from(stars))])
}

/// The password `user_model_with_password` and `user_registration` use.
pub const PASSWORD: &str = "Passw0rd!";

/// Hashes `password` with argon2 and the fixed test salt, as `users::register` does under test.
pub fn password_hash(password: &str) -> String {
    use argon2::{Argon2, PasswordHasher};
    use crate::infrastructure::Random;
    Argon2::default()
        .hash_password(password.as_bytes(), &SeqRandom::new().salt())
        .expect("hashing a test password")
        .to_string()
}

/// Like `user_model`, but with the real argon2 hash of `password`.
pub fn user_model_with_password(id: i32, password: &str) -> users::Model {
    users::Model { password_hash: password_hash(password), ..user_model(id) }
}

/// A valid sign-up request (password `PASSWORD`). Tests override fields with struct-update syntax.
pub fn user_registration() -> UserRegistration {
    UserRegistration {
        email: "new@example.test".into(),
        username: "newuser".into(),
        password: PASSWORD.into(),
        password_repeat: PASSWORD.into(),
    }
}

pub fn app_token_model(uuid: Uuid, name: &str, user_id: i32) -> app_tokens::Model {
    app_tokens::Model {
        uuid,
        name: name.into(),
        user_id,
        generated: date().and_hms_opt(0, 0, 0).expect("valid time"),
    }
}

pub fn manual_image_reference_model(image_id: i32, media_id: i32) -> manual_image_references::Model {
    manual_image_references::Model { image_id, media_id }
}

pub fn watchlist_model(media_id: i32) -> watchlist::Model {
    watchlist::Model { media_id, assessment: None, date_added: date() }
}

/// A `media_locks` row with nothing locked. Override fields to lock them.
pub fn media_locks_model(media_id: i32) -> media_locks::Model {
    media_locks::Model {
        media_id,
        backdrop_path: false,
        homepage: false,
        imdb_id: false,
        overview: false,
        poster_path: false,
        tmdb_vote_average: false,
        original_language: false,
        origin_country: false,
    }
}

/// A `movie_locks` row with nothing locked. Override fields to lock them.
pub fn movie_locks_model(movie_id: i32) -> movie_locks::Model {
    movie_locks::Model { movie_id, release_date: false, runtime: false, status: false }
}

/// A series `media` row owned by `user_id`.
pub fn series_media_model(id: i32, user_id: i32) -> media::Model {
    media_model(id, user_id, MediaType::Series)
}

/// A `series_locks` row with nothing locked. Override fields to lock them.
pub fn series_locks_model(series_id: i32) -> series_locks::Model {
    series_locks::Model { series_id, first_air_date: false, status: false, r#type: false }
}

/// Season `season_number` of series `series_id`, with no poster.
pub fn season_model(id: i32, series_id: i32, season_number: i32) -> seasons::Model {
    seasons::Model {
        id,
        series_id,
        air_date: Some(date()),
        episode_count: Some(0),
        name: Some(format!("Season {season_number}")),
        overview: Some(format!("Overview of season {season_number}")),
        season_number: Some(season_number),
        tmdb_vote_average: Some(7.5),
        stars: None,
        tmdb_id: Some(season_number),
        poster_image_id: None,
    }
}

/// Episode `episode_number` of season `season_id` with TMDB id `tmdb_id`, with no still.
pub fn episode_model(id: i32, season_id: i32, tmdb_id: i32, episode_number: i32) -> episodes::Model {
    episodes::Model {
        id,
        season_id,
        tmdb_id: Some(tmdb_id),
        episode_number: Some(episode_number),
        name: Some(format!("Episode {episode_number}")),
        overview: Some(format!("Overview of episode {episode_number}")),
        air_date: Some(date()),
        runtime: Some(45),
        tmdb_vote_average: Some(7.5),
        still_image_id: None,
    }
}

/// A row of the media index query (`find_also_linked(MediaPosters).find_also_related(Titles)`):
/// a media row with its optional poster image and optional title. Queue it with
/// `append_query_results([vec![media_index_row(..)]])`.
pub fn media_index_row(media: media::Model, poster: Option<images::Model>, title: Option<titles::Model>) -> BTreeMap<String, Value> {
    let mut row = BTreeMap::new();
    for column in media::Column::iter() {
        row.insert(format!("A_{}", column.as_str()), media.get(column));
    }
    if let Some(poster) = poster {
        for column in images::Column::iter() {
            row.insert(format!("B_{}", column.as_str()), poster.get(column));
        }
    }
    if let Some(title) = title {
        for column in titles::Column::iter() {
            row.insert(format!("C_{}", column.as_str()), title.get(column));
        }
    }
    row
}

/// A message of `exchange_id` with the given `role` and Anthropic `content` blocks, no LLM call,
/// created at a fixed time.
pub fn message_model(id: i32, exchange_id: i32, role: MessageRole, content: serde_json::Value) -> messages::Model {
    messages::Model {
        id,
        exchange_id,
        role,
        content,
        llm_call_id: None,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time").fixed_offset(),
    }
}

/// An Exchange of `conversation_id` with the given `status`, no error, created at a fixed time.
pub fn exchange_model(id: i32, conversation_id: i32, status: ExchangeStatus) -> exchanges::Model {
    exchanges::Model {
        id,
        conversation_id,
        status,
        error: None,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time").fixed_offset(),
        completed_at: None,
    }
}

/// A titled Conversation of `user_id`, created and updated at a fixed time.
pub fn conversation_model(id: i32, user_id: i32) -> conversations::Model {
    let at = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time").fixed_offset();
    conversations::Model { id, user_id, title: Some("A conversation".into()), created_at: at, updated_at: at }
}

/// An open `exchange` LLM call of `exchange_id` (user 1, conversation 3) to `test/model`: no
/// result yet, created at a fixed time.
pub fn llm_call_model(id: i32, exchange_id: i32) -> llm_calls::Model {
    llm_calls::Model {
        id,
        user_id: Some(1),
        conversation_id: Some(3),
        exchange_id: Some(exchange_id),
        purpose: LlmCallPurpose::Exchange,
        model: "test/model".into(),
        stop_reason: None,
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_creation_tokens: None,
        provider_request_id: None,
        error: None,
        created_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).single().expect("valid time").fixed_offset(),
        completed_at: None,
    }
}
