//! Media search against a real Postgres: one test per group of search syntax the `transpiler`
//! supports, so every kind of SQL it generates runs at least once. Needs Docker:
//! `cargo test -p services --test postgres_search -- --ignored`.
//!
//! Query syntax: `<title prefix> : <expression> : <sort target> [asc|desc]`.

mod common;

use chrono::NaiveDate;
use common::postgres::database;
use common::seed;
use entities::sea_orm_active_enums::MediaType;
use entities::{media, movies, series};
use integrations::tmdb::views::MultiResults;
use sea_orm::{ActiveModelTrait, DbConn, Set};
use services::test_support::tmdb::movie_details;
use services::test_support::MockTmdb;
use services::{media as media_service, watchlist};
use views::media::{MediaByIdsQuery, MediaType as ViewMediaType, PageReq, SearchQuery, SearchResults, SortTarget};
use views::users::CurrentUser;

const ALIEN: &str = "Alien";
const DARK: &str = "Dark";
const MATRIX: &str = "The Matrix";

/// What the searches run against.
struct Library {
    user: CurrentUser,
    alien: i32,
}

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).expect("valid date")
}

/// Three media of one user, plus another user's movie that no search may return:
///
/// | title      | type   | stars | TMDB rating | language | released   | added      | sources  | logs | tags    | genre  | watchlist | status           |
/// |------------|--------|-------|-------------|----------|------------|------------|----------|------|---------|--------|-----------|------------------|
/// | The Matrix | movie  | 8.5   | 8.2         | en       | 1999-03-31 | 2024-01-01 | Jellyfin | 2    | Classic | Action | no        | Released         |
/// | Alien      | movie  | 6     | 7.9         | none     | 1979-05-25 | 2024-03-01 | none     | 0    | none    | Horror | yes       | none             |
/// | Dark       | series | none  | 8.4         | de       | 2017-12-01 | 2024-02-01 | Netflix  | 1    | none    | Drama  | no        | Returning Series |
///
/// The logs: The Matrix on 2024-02-10 ("Great REWATCH of 100% fun") and 2024-05-01 (no comment); Dark on 2024-03-15 ("slow burn").
async fn library(db: &DbConn) -> Library {
    seed::language(db, "en", "English").await;
    seed::language(db, "de", "German").await;
    let user = seed::user(db, 1).await;

    let matrix = seed::movie(db, &user, 603, MATRIX).await;
    describe(db, matrix.id, Some(8.5), 8.2, Some("en"), day(2024, 1, 1)).await;
    release(db, matrix.id, MediaType::Movie, day(1999, 3, 31)).await;
    let jellyfin = seed::source(db, matrix.id, "Jellyfin").await;
    seed::log_on(db, matrix.id, jellyfin.id, day(2024, 2, 10), Some("Great REWATCH of 100% fun")).await;
    seed::log_on(db, matrix.id, jellyfin.id, day(2024, 5, 1), None).await;
    set_status(db, matrix.id, MediaType::Movie, "Released").await;
    seed::tag(db, matrix.id, "Classic").await;
    seed::genre(db, matrix.id, "Action", MediaType::Movie, Some(28)).await;

    let alien = seed::movie(db, &user, 348, ALIEN).await;
    describe(db, alien.id, Some(6.0), 7.9, None, day(2024, 3, 1)).await;
    release(db, alien.id, MediaType::Movie, day(1979, 5, 25)).await;
    seed::genre(db, alien.id, "Horror", MediaType::Movie, Some(27)).await;
    seed::on_watchlist(db, alien.id).await;

    let dark = seed::series(db, &user, 70523, DARK).await;
    describe(db, dark.id, None, 8.4, Some("de"), day(2024, 2, 1)).await;
    release(db, dark.id, MediaType::Series, day(2017, 12, 1)).await;
    let netflix = seed::source(db, dark.id, "Netflix").await;
    seed::log_on(db, dark.id, netflix.id, day(2024, 3, 15), Some("slow burn")).await;
    set_status(db, dark.id, MediaType::Series, "Returning Series").await;
    seed::genre(db, dark.id, "Drama", MediaType::Series, Some(18)).await;

    let other = seed::user(db, 2).await;
    let other_matrix = seed::movie(db, &other, 604, "The Matrix Reloaded").await;
    describe(db, other_matrix.id, Some(9.0), 7.0, Some("en"), day(2024, 1, 1)).await;

    Library { user, alien: alien.id }
}

async fn describe(db: &DbConn, id: i32, stars: Option<f32>, tmdb_rating: f32, language: Option<&str>, added: NaiveDate) {
    seed::update_media(db, id, media::ActiveModel {
        stars: Set(stars),
        tmdb_vote_average: Set(Some(tmdb_rating)),
        original_language: Set(language.map(str::to_string)),
        date_added: Set(added),
        ..Default::default()
    }).await;
}

async fn release(db: &DbConn, id: i32, media_type: MediaType, date: NaiveDate) {
    match media_type {
        MediaType::Movie => movies::ActiveModel { id: Set(id), release_date: Set(Some(date)), ..Default::default() }
            .update(db).await.map(|_| ()),
        MediaType::Series => series::ActiveModel { id: Set(id), first_air_date: Set(Some(date)), ..Default::default() }
            .update(db).await.map(|_| ()),
    }.expect("setting the release date");
}

async fn set_status(db: &DbConn, id: i32, media_type: MediaType, status: &str) {
    match media_type {
        MediaType::Movie => movies::ActiveModel { id: Set(id), status: Set(Some(status.into())), ..Default::default() }
            .update(db).await.map(|_| ()),
        MediaType::Series => series::ActiveModel { id: Set(id), status: Set(Some(status.into())), ..Default::default() }
            .update(db).await.map(|_| ()),
    }.expect("setting the status");
}

fn query(text: &str) -> SearchQuery {
    SearchQuery { query: text.into(), genres: None, only_watched: false, min_stars: None, sort_by: SortTarget::Title }
}

fn all() -> PageReq {
    PageReq { offset: None, limit: None }
}

async fn run(search: SearchQuery, library: &Library, db: &DbConn, tmdb: &MockTmdb) -> SearchResults {
    let text = search.query.clone();
    let results = media_service::search(search, false, all(), None, &library.user, db, tmdb).await
        .ok().unwrap_or_else(|| panic!("search {text:?} failed"));
    assert!(results.query_valid, "search {text:?} should be valid");
    results
}

/// The titles `text` finds, in result order.
async fn titles(text: &str, library: &Library, db: &DbConn) -> Vec<String> {
    titles_of(run(query(text), library, db, &MockTmdb::new()).await)
}

fn titles_of(results: SearchResults) -> Vec<String> {
    results.indexes.into_iter().map(|index| index.title).collect()
}

/// Asserts what each query finds, in default order (title, ignoring a leading "The"/"A").
async fn assert_finds(cases: &[(&str, &[&str])], library: &Library, db: &DbConn) {
    for (text, expected) in cases {
        assert_eq!(titles(text, library, db).await, *expected, "search {text:?}");
    }
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_title_prefix_when_query_is_a_name() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        ("Mat", &[MATRIX]),
        ("ali", &[ALIEN]),
        ("x", &[]),
        ("", &[ALIEN, DARK, MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_numeric_comparisons() {
    let db = database().await;
    let library = library(&db).await;
    let alien_id = format!(" : id = {}", library.alien);

    assert_finds(&[
        (" : stars >= 7", &[MATRIX]),
        (" : stars < 7", &[ALIEN]),
        (" : stars = 6", &[ALIEN]),
        (" : stars != 6", &[DARK, MATRIX]),
        (" : stars <= 8.5", &[ALIEN, MATRIX]),
        (" : tmdb_rating > 8", &[DARK, MATRIX]),
        (" : tmdb_rating <= 8", &[ALIEN]),
        (&alien_id, &[ALIEN]),
        (" : tmdb_id = 70523", &[DARK]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_boolean_targets() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (" : watched = true", &[DARK, MATRIX]),
        (" : watched = false", &[ALIEN]),
        (" : rated = true", &[ALIEN, MATRIX]),
        (" : rated = false", &[DARK]),
        (" : on_watchlist = true", &[ALIEN]),
        (" : on_watchlist = false", &[DARK, MATRIX]),
        (" : has_source = true", &[DARK, MATRIX]),
        (" : has_source = false", &[ALIEN]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_name_targets() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (r#" : has_tag = "classic""#, &[MATRIX]),
        (r#" : has_genre = "Drama""#, &[DARK]),
        (r#" : has_source_with_name = "jellyfin""#, &[MATRIX]),
        (r#" : language = "german""#, &[DARK]),
        (r#" : language = "en""#, &[MATRIX]),
        (r#" : type = "series""#, &[DARK]),
        (r#" : type = "movie""#, &[ALIEN, MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_date_targets() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (" : release_date < 1990-01-01", &[ALIEN]),
        (" : release_date >= 1999-03-31", &[DARK, MATRIX]),
        (" : date_added > 2024-01-15", &[ALIEN, DARK]),
        (" : date_added = 2024-01-01", &[MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_count_targets() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (" : times_watched >= 2", &[MATRIX]),
        (" : times_watched = 0", &[ALIEN]),
        (" : number_of_sources = 1", &[DARK, MATRIX]),
        (" : number_of_sources < 1", &[ALIEN]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_watched_date_targets() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (" : first_watched < 2024-03-01", &[MATRIX]),
        (" : first_watched >= 2024-03-01", &[DARK]),
        (" : first_watched = 2024-02-10", &[MATRIX]),
        (" : last_watched = 2024-05-01", &[MATRIX]),
        (" : last_watched > 2024-04-01", &[MATRIX]),
        (" : last_watched < 2024-04-01", &[DARK]),
        (" : last_watched != 2024-05-01", &[ALIEN, DARK]),
        (" : 2024-03-01 <= last_watched <= 2024-12-31", &[DARK, MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_status_target() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (r#" : status = "released""#, &[MATRIX]),
        (r#" : status = "RETURNING SERIES""#, &[DARK]),
        (r#" : status = "ended""#, &[]),
        (r#" : status != "released""#, &[ALIEN, DARK]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_log_comment_target() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (r#" : has_log_containing = "rewatch""#, &[MATRIX]),
        (r#" : has_log_containing = "SLOW""#, &[DARK]),
        (r#" : has_log_containing = "100%""#, &[MATRIX]),
        (r#" : has_log_containing = "10%fun""#, &[]),
        (r#" : has_log_containing = "_ewatch""#, &[]),
        (r#" : has_log_containing = "burn" or has_log_containing = "fun""#, &[DARK, MATRIX]),
        (r#" : has_log_containing != "rewatch""#, &[ALIEN, DARK]),
        (r#" : has_log_containing != "zzz""#, &[ALIEN, DARK, MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_matches_ternary_comparisons() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (" : 5 < stars < 9", &[ALIEN, MATRIX]),
        (" : 7 <= tmdb_rating <= 8.3", &[ALIEN, MATRIX]),
        (" : 2000-01-01 <= release_date <= 2020-01-01", &[DARK]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_combines_logical_operators() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (r#" : type = "movie" and stars > 7"#, &[MATRIX]),
        (r#" : has_genre = "horror" or has_genre = "drama""#, &[ALIEN, DARK]),
        (" : (stars > 7 or rated = false) and has_source = true", &[DARK, MATRIX]),
        ("Ma : watched = true", &[MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_orders_by_sort_target() {
    let db = database().await;
    let library = library(&db).await;

    assert_finds(&[
        (" :  : stars desc", &[MATRIX, ALIEN, DARK]),
        (" :  : stars", &[DARK, ALIEN, MATRIX]),
        (" :  : tmdb_rating desc", &[DARK, MATRIX, ALIEN]),
        (" :  : times_watched descending", &[MATRIX, DARK, ALIEN]),
        (" :  : watched asc", &[ALIEN, DARK, MATRIX]),
        (" :  : release_date", &[ALIEN, MATRIX, DARK]),
        (" :  : date_added desc", &[ALIEN, DARK, MATRIX]),
        (" :  : last_watched desc", &[MATRIX, DARK, ALIEN]),
        (" :  : last_watched", &[ALIEN, DARK, MATRIX]),
        (" :  : first_watched desc", &[DARK, MATRIX, ALIEN]),
        (" :  : type", &[ALIEN, MATRIX, DARK]),
        (" :  : has_source", &[ALIEN, DARK, MATRIX]),
        (" :  : number_of_sources desc", &[DARK, MATRIX, ALIEN]),
        (r#" : type = "movie" : stars"#, &[ALIEN, MATRIX]),
    ], &library, &db).await;
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_applies_search_options() {
    let db = database().await;
    let library = library(&db).await;
    let tmdb = MockTmdb::new();

    let cases: Vec<(SearchQuery, &[&str])> = vec![
        (SearchQuery { genres: Some(vec!["horror".into()]), ..query("") }, &[ALIEN]),
        (SearchQuery { only_watched: true, ..query("") }, &[DARK, MATRIX]),
        (SearchQuery { min_stars: Some(7.0), ..query("") }, &[MATRIX]),
        (SearchQuery { sort_by: SortTarget::Stars, ..query("") }, &[MATRIX, ALIEN, DARK]),
        (SearchQuery { sort_by: SortTarget::TimesWatched, ..query("") }, &[MATRIX, DARK, ALIEN]),
    ];
    for (search, expected) in cases {
        let description = format!("genres {:?}, only watched {}, min stars {:?}", search.genres, search.only_watched, search.min_stars);
        assert_eq!(titles_of(run(search, &library, &db, &tmdb).await), expected, "{description}");
    }

    let series_only = media_service::search(query(""), false, all(), Some(ViewMediaType::Series), &library.user, &db, &tmdb).await
        .ok().expect("search should succeed");
    assert_eq!(titles_of(series_only), vec![DARK.to_string()]);

    let paged = media_service::search(query(""), false, PageReq { offset: Some(1), limit: Some(1) }, None, &library.user, &db, &tmdb).await
        .ok().expect("search should succeed");
    assert_eq!(titles_of(paged), vec![DARK.to_string()]);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_returns_external_movie_when_query_is_tmdb_id_and_type() {
    let db = database().await;
    let library = library(&db).await;
    let mut tmdb = MockTmdb::new();
    tmdb.expect_movie_details().withf(|id| *id == 999).times(1).returning(|id| Ok(movie_details(id)));

    let results = run(query(r#" : tmdb_id = 999 and type = "movie""#), &library, &db, &tmdb).await;

    assert!(results.indexes.is_empty());
    let external: Vec<(i32, String)> = results.external.into_iter().map(|e| (e.external_id, e.title)).collect();
    assert_eq!(external, vec![(999, "Movie 999".to_string())]);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_asks_tmdb_when_name_is_longer_than_three_characters() {
    let db = database().await;
    let library = library(&db).await;
    let mut tmdb = MockTmdb::new();
    tmdb.expect_search_multi().withf(|name| name == "Matr").times(1).returning(|_| Ok(MultiResults::default()));

    let results = run(query("Matr"), &library, &db, &tmdb).await;

    assert_eq!(titles_of(results), vec![MATRIX.to_string()]);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn search_by_ids_returns_matching_media_among_ids() {
    let db = database().await;
    let library = library(&db).await;

    let results = media_service::search_by_ids(MediaByIdsQuery { query: " : stars > 1".into(), ids: vec![library.alien] }, all(), &library.user, &db).await
        .ok().expect("search should succeed");

    assert!(results.query_valid);
    let titles: Vec<String> = results.indexes.into_iter().map(|index| index.title).collect();
    assert_eq!(titles, vec![ALIEN.to_string()]);
}

#[tokio::test]
#[ignore = "requires docker"]
async fn watchlist_search_returns_only_watchlisted_media() {
    let db = database().await;
    let library = library(&db).await;

    let results = watchlist::search(query(" : stars > 1"), all(), &library.user, &db).await.ok().expect("search should succeed");

    assert_eq!(titles_of(results), vec![ALIEN.to_string()]);
}
