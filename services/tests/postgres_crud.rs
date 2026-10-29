//! One create-read-update-delete round trip per resource against a real Postgres. Needs Docker:
//! `cargo test -p services --test postgres_crud -- --ignored`.

mod common;

use chrono::{TimeZone, Utc};
use common::postgres::database;
use common::seed;
use entities::prelude::{Genres, Languages, MediaGenres, MediaTags, Tags, Titles};
use entities::sea_orm_active_enums::MediaType;
use entities::{genres, media, titles};
use integrations::tmdb::views::Languages as TmdbLanguage;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter, QueryOrder};
use services::infrastructure::{RuleViolation, SrvErr};
use services::test_support::fixtures::{user_registration, PASSWORD};
use services::test_support::tmdb::genre_list;
use services::test_support::{assert_single_violation, fixed_clock, violations, MockTmdb, SeqRandom};
use services::{app_tokens, genres as genres_service, logs, masterdata, sources, tags as tags_service, titles as titles_service, users, watchlist};
use uuid::Uuid;
use views::genres::GenreCreate;
use views::logs::{LogCreate, LogUpdate};
use views::media::{MediaType as ViewMediaType, PageReq};
use views::sources::{SourceCreate, SourceType, SourceUpdate};
use views::tags::TagCreate;
use views::titles::TitleCreate;
use views::users::UserLogin;

const MOVIE: ViewMediaType = ViewMediaType::Movie;

async fn stars(db: &DbConn, media_id: i32) -> Option<f32> {
    media::Entity::find_by_id(media_id).one(db).await.expect("querying media").expect("media row").stars
}

/// `(title, primary)` of the media's titles, by id.
async fn titles_of(db: &DbConn, media_id: i32) -> Vec<(String, bool)> {
    Titles::find().filter(titles::Column::MediaId.eq(media_id)).order_by_asc(titles::Column::Id).all(db).await
        .expect("querying titles").into_iter().map(|t| (t.title, t.primary)).collect()
}

fn all() -> PageReq {
    PageReq { offset: None, limit: None }
}

#[tokio::test]
#[ignore = "requires docker"]
async fn tags_create_and_delete_round_trip() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let first = seed::movie(&db, &user, 1, "First").await;
    let second = seed::movie(&db, &user, 2, "Second").await;

    let created = tags_service::create(first.id, TagCreate { name: "cult classic".into() }, MOVIE, &user, &db).await.ok().expect("create");
    let again = tags_service::create(first.id, TagCreate { name: "Cult Classic".into() }, MOVIE, &user, &db).await.ok().expect("create again");
    let reused = tags_service::create(second.id, TagCreate { name: "cult classic".into() }, MOVIE, &user, &db).await.ok().expect("create on second");

    assert_eq!((created, again, reused), (true, false, true));
    let tags: Vec<String> = Tags::find().all(&db).await.expect("querying tags").into_iter().map(|t| t.name).collect();
    assert_eq!(tags, vec!["Cult Classic".to_string()]);
    assert_eq!(seed::count(&db, MediaTags).await, 2);
    let tag_id = Tags::find().one(&db).await.expect("querying tags").expect("the tag").id;

    tags_service::delete(first.id, tag_id, MOVIE, &user, &db).await.ok().expect("delete from first");
    assert_eq!((seed::count(&db, MediaTags).await, seed::count(&db, Tags).await), (1, 1), "the tag stays while the second movie has it");
    tags_service::delete(second.id, tag_id, MOVIE, &user, &db).await.ok().expect("delete from second");
    assert_eq!((seed::count(&db, MediaTags).await, seed::count(&db, Tags).await), (0, 0), "the unused tag is deleted");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn genres_create_index_and_delete_round_trip() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let movie = seed::movie(&db, &user, 1, "Movie").await;

    genres_service::create(movie.id, &GenreCreate { name: "space opera".into() }, MOVIE, &user, &db).await.ok().expect("create");

    let index = genres_service::index(Some(MOVIE), &user, &db).await.ok().expect("index");
    assert_eq!(index, vec!["Space Opera".to_string()]);
    let genre = Genres::find().filter(genres::Column::Name.eq("Space Opera")).one(&db).await.expect("querying genres").expect("the genre");
    assert_eq!((genre.tmdb_id, genre.r#type), (None, MediaType::Movie));

    genres_service::delete(movie.id, genre.id, MOVIE, &user, &db).await.ok().expect("delete");
    assert_eq!((seed::count(&db, MediaGenres).await, seed::count(&db, Genres).await), (0, 0), "the unused user genre is deleted");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn titles_create_set_primary_and_delete_round_trip() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let movie = seed::movie(&db, &user, 1, "Original").await;

    assert!(titles_service::create(movie.id, &TitleCreate { name: "Alternative".into() }, MOVIE, &user, &db).await.ok().expect("create"));
    assert!(!titles_service::create(movie.id, &TitleCreate { name: "Alternative".into() }, MOVIE, &user, &db).await.ok().expect("create again"));
    assert_eq!(titles_of(&db, movie.id).await, vec![("Original".to_string(), true), ("Alternative".to_string(), false)]);
    let alternative_id = Titles::find().filter(titles::Column::Title.eq("Alternative")).one(&db).await.expect("querying titles").expect("title").id;

    titles_service::set_primary(movie.id, alternative_id, MOVIE, &user, &db).await.ok().expect("set primary");
    assert_eq!(titles_of(&db, movie.id).await, vec![("Original".to_string(), false), ("Alternative".to_string(), true)]);

    titles_service::delete(movie.id, alternative_id, MOVIE, &user, &db).await.ok().expect("delete");
    assert_eq!(titles_of(&db, movie.id).await, vec![("Original".to_string(), true)], "the remaining title becomes primary");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn sources_create_read_update_and_delete_round_trip() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let movie = seed::movie(&db, &user, 1, "Movie").await;
    let create = SourceCreate { name: "Jellyfin".into(), url: "https://jellyfin.example".into(), r#type: SourceType::Jellyfin };

    sources::create(movie.id, &create, MOVIE, &user, &db).await.ok().expect("create");
    assert_single_violation(sources::create(movie.id, &create, MOVIE, &user, &db).await,
                            |v| matches!(v, RuleViolation::SourceNameAlreadyExists));

    let index = sources::index(movie.id, MOVIE, &user, &db).await.ok().expect("index");
    assert_eq!(index.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Jellyfin"]);
    let id = index[0].id;
    let update = SourceUpdate { id, name: "Web".into(), url: "https://web.example".into(), r#type: SourceType::Web };
    sources::update(movie.id, id, &update, MOVIE, &user, &db).await.ok().expect("update");
    let details = sources::details(movie.id, MOVIE, id, &user, &db).await.ok().expect("details");
    assert_eq!((details.name.as_str(), details.url.as_str(), matches!(details.r#type, SourceType::Web)), ("Web", "https://web.example", true));

    let log = seed::log(&db, movie.id, id, Some(7.0), true).await;
    assert_single_violation(sources::delete(movie.id, id, MOVIE, &user, &db).await, |v| matches!(v, RuleViolation::SourceHasLogs));
    logs::delete(movie.id, log.id, MOVIE, &user, &db).await.ok().expect("deleting the log");
    sources::delete(movie.id, id, MOVIE, &user, &db).await.ok().expect("delete");
    assert!(sources::index(movie.id, MOVIE, &user, &db).await.ok().expect("index").is_empty());
}

#[tokio::test]
#[ignore = "requires docker"]
async fn logs_create_update_and_delete_round_trip_keep_media_rating() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let movie = seed::movie(&db, &user, 1, "Movie").await;
    seed::source(&db, movie.id, "Jellyfin").await;
    let create = |stars| LogCreate { date: seed::date_added(), stars, completed: true, comment: Some("Fine".into()), source: "Jellyfin".into() };

    logs::create(movie.id, &create(Some(6.0)), MOVIE, &user, &db).await.ok().expect("create first");
    logs::create(movie.id, &create(Some(9.0)), MOVIE, &user, &db).await.ok().expect("create second");
    assert_eq!(stars(&db, movie.id).await, Some(7.5));

    let first = entities::logs::Entity::find().order_by_asc(entities::logs::Column::Id).one(&db).await.expect("querying logs").expect("a log");
    let update = LogUpdate { id: first.id, date: seed::date_added(), stars: Some(8.0), completed: false, comment: None, source: "Jellyfin".into() };
    logs::update(movie.id, first.id, &update, MOVIE, &user, &db).await.ok().expect("update");
    assert_eq!(stars(&db, movie.id).await, Some(8.5));
    let updated = entities::logs::Entity::find_by_id(first.id).one(&db).await.expect("querying logs").expect("the log");
    assert_eq!((updated.stars, updated.completed, updated.comment), (Some(8.0), false, None));

    for log in entities::logs::Entity::find().all(&db).await.expect("querying logs") {
        logs::delete(movie.id, log.id, MOVIE, &user, &db).await.ok().expect("delete");
    }
    assert_eq!(stars(&db, movie.id).await, None);
}

#[tokio::test]
#[ignore = "known bug: logs::details never loads the source, so Log.source is always empty"]
async fn logs_details_returns_source_name() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let movie = seed::movie(&db, &user, 1, "Movie").await;
    let source = seed::source(&db, movie.id, "Jellyfin").await;
    let log = seed::log(&db, movie.id, source.id, Some(7.0), true).await;

    let details = logs::details(movie.id, MOVIE, log.id, &user, &db).await.ok().expect("details");

    assert_eq!((details.id, details.stars), (log.id, Some(7.0)));
    assert_eq!(details.source, "Jellyfin");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn watchlist_add_index_and_remove_round_trip() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let movie = seed::movie(&db, &user, 1, "Movie").await;
    seed::movie(&db, &user, 2, "Not Listed").await;
    let clock = fixed_clock(2024, 2, 1, 12, 0, 0);

    assert!(watchlist::add(movie.id, &user, &db, &clock).await.ok().expect("add"));
    assert!(!watchlist::add(movie.id, &user, &db, &clock).await.ok().expect("add again"));
    let index = watchlist::index(all(), &user, &db).await.ok().expect("index");
    assert_eq!(index.iter().map(|i| i.title.as_str()).collect::<Vec<_>>(), vec!["Movie"]);

    watchlist::remove(movie.id, &user, &db).await.ok().expect("remove");
    assert!(watchlist::index(all(), &user, &db).await.ok().expect("index").is_empty());
    assert!(matches!(watchlist::remove(movie.id, &user, &db).await, Err(SrvErr::NotFound)));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn app_tokens_generate_index_and_revoke_round_trip() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let clock = fixed_clock(2024, 2, 1, 12, 0, 0);
    let random = SeqRandom::new();
    let secret = "test-secret".to_string();

    let token = app_tokens::generate("Jellyfin".into(), &secret, &user, &db, &clock, &random).await.ok().expect("generate");

    assert!(!token.token.is_empty());
    let index = app_tokens::index(&user, &db).await.ok().expect("index");
    let index: Vec<(String, chrono::DateTime<Utc>)> = index.into_iter().map(|t| (t.name, t.generated)).collect();
    assert_eq!(index, vec![("Jellyfin".to_string(), Utc.with_ymd_and_hms(2024, 2, 1, 12, 0, 0).unwrap())]);
    let owner = users::query_user_by_app_token(Uuid::from_u128(1000), &db).await.ok().expect("query").expect("the token's user");
    assert_eq!(owner.id, user.id);

    app_tokens::revoke("Jellyfin".into(), &user, &db).await.ok().expect("revoke");
    assert!(app_tokens::index(&user, &db).await.ok().expect("index").is_empty());
    assert!(users::query_user_by_app_token(Uuid::from_u128(1000), &db).await.ok().expect("query").is_none());
}

#[tokio::test]
#[ignore = "requires docker"]
async fn users_register_then_login_round_trip() {
    let db = database().await;
    let random = SeqRandom::with_uuids(vec![Uuid::from_u128(42)]);
    let clock = fixed_clock(2024, 2, 1, 12, 0, 0);
    let secret = "test-secret".to_string();

    users::register(&user_registration(), &db, &random).await.ok().expect("register");
    let duplicate = violations(users::register(&user_registration(), &db, &random).await);
    assert!(matches!(duplicate.as_slice(),
        [RuleViolation::SignUpAccountWithThisEmailAlreadyExists, RuleViolation::SignUpUsernameAlreadyTaken]),
        "unexpected violations: {duplicate:?}");

    let token = users::login(&UserLogin { username: "newuser".into(), password: PASSWORD.into() }, &secret, &db, &clock).await
        .ok().expect("login");
    assert!(!token.token.is_empty());
    assert!(!token.admin);
    assert_eq!(token.expiry_date, Utc.with_ymd_and_hms(2024, 4, 1, 12, 0, 0).unwrap());
    let wrong = users::login(&UserLogin { username: "newuser".into(), password: "Wrong1pass".into() }, &secret, &db, &clock).await;
    assert!(matches!(wrong, Err(SrvErr::Unauthorized)));
    let user = users::query_user_by_uuid(Uuid::from_u128(42), &db).await.ok().expect("query").expect("the registered user");
    assert_eq!((user.username.as_str(), user.email.as_str()), ("newuser", "new@example.test"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn masterdata_refresh_inserts_then_updates_languages_and_genres() {
    let db = database().await;
    let refresh = |language_name: &'static str, movie_genre: &'static str| {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_languages().times(1).returning(move || Ok(vec![
            TmdbLanguage { iso_639_1: "en".into(), english_name: "English".into(), name: language_name.into() },
        ]));
        tmdb.expect_movie_genres().times(1).returning(move || Ok(genre_list(&[(28, movie_genre)])));
        tmdb.expect_tv_genres().times(1).returning(|| Ok(genre_list(&[(18, "Drama")])));
        tmdb
    };

    masterdata::refresh(&db, &refresh("Englisch", "Action")).await.ok().expect("first refresh");
    masterdata::refresh(&db, &refresh("English", "Action & Adventure")).await.ok().expect("second refresh");

    let languages: Vec<(String, String)> = Languages::find().all(&db).await.expect("querying languages")
        .into_iter().map(|l| (l.iso6391, l.name)).collect();
    assert_eq!(languages, vec![("en".to_string(), "English".to_string())]);
    let genres: Vec<(Option<i32>, String, MediaType)> = Genres::find().order_by_asc(genres::Column::TmdbId).all(&db).await
        .expect("querying genres").into_iter().map(|g| (g.tmdb_id, g.name, g.r#type)).collect();
    assert_eq!(genres, vec![
        (Some(18), "Drama".to_string(), MediaType::Series),
        (Some(28), "Action & Adventure".to_string(), MediaType::Movie),
    ]);
}
