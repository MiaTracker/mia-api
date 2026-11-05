//! The TMDB refresh and the ratings linearization against a real Postgres. Needs Docker:
//! `cargo test -p services --test postgres_refresh -- --ignored`.

mod common;

use std::sync::Arc;
use chrono::{TimeZone, Utc};
use common::postgres::database;
use common::{fakes, seed};
use entities::prelude::{Logs, Media, Movies, SyncState, Titles};
use entities::{logs, sync_state, titles};
use integrations::tmdb::views::PropertyChanges;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, NotSet, QueryFilter, QueryOrder, Set};
use serde_json::json;
use services::ratings_linearization::linearize_ratings;
use services::refresh::refresh;
use services::test_support::tmdb::property_changes;
use services::test_support::{date, fixed_clock, MockTmdb};

/// Property changes with every item in English.
fn english_changes(changes: &[(&str, serde_json::Value)]) -> PropertyChanges {
    let mut changes = property_changes(changes);
    for change in &mut changes.changes {
        for item in &mut change.items {
            item.iso_639_1 = Some("en".into());
        }
    }
    changes
}

#[tokio::test]
#[ignore = "requires docker"]
async fn refresh_applies_movie_property_changes() {
    let shared = Arc::new(database().await);
    let db: &DbConn = &shared;
    let f = fakes(&shared);
    let user = seed::user(db, 1).await;
    let movie = seed::movie(db, &user, 603, "The Matrix").await;
    let clock = fixed_clock(2024, 1, 5, 12, 0, 0);
    let mut tmdb = MockTmdb::new();
    tmdb.expect_movie_changed().withf(|start, end| (*start, *end) == (date(2024, 1, 1), date(2024, 1, 5)))
        .times(1).returning(|_, _| Ok(vec![603, 999]));
    tmdb.expect_series_changed().times(1).returning(|_, _| Ok(Vec::new()));
    tmdb.expect_movie_property_changes().withf(|id, start, end| (*id, *start, *end) == (603, date(2024, 1, 1), date(2024, 1, 5)))
        .times(1).returning(|_, _, _| Ok(english_changes(&[
            ("overview", json!("A new overview")),
            ("runtime", json!(136)),
            ("status", json!("Released")),
            ("title", json!("Matrix")),
        ])));

    let result = refresh(db, &tmdb, &f.store, &f.jobs, &clock, &f.random).await.expect("refresh should succeed");

    assert_eq!((result.updates, result.errors), (1, 0));
    let media = Media::find_by_id(movie.id).one(db).await.expect("querying media").expect("media row");
    assert_eq!(media.overview.as_deref(), Some("A new overview"));
    let row = Movies::find_by_id(movie.id).one(db).await.expect("querying movies").expect("movie row");
    assert_eq!((row.runtime, row.status.as_deref()), (Some(136), Some("Released")));
    let titles: Vec<(String, bool)> = Titles::find().filter(titles::Column::MediaId.eq(movie.id)).order_by_asc(titles::Column::Id)
        .all(db).await.expect("querying titles").into_iter().map(|t| (t.title, t.primary)).collect();
    assert_eq!(titles, vec![("The Matrix".to_string(), true), ("Matrix".to_string(), false)]);
    let synced: Vec<_> = SyncState::find().all(db).await.expect("querying sync state").into_iter().map(|s| s.synced_at).collect();
    assert_eq!(synced, vec![Utc.with_ymd_and_hms(2024, 1, 5, 12, 0, 0).unwrap().fixed_offset()]);
}

/// The last sync was more than 14 days ago, so every movie is refreshed, in 14-day windows.
#[tokio::test]
#[ignore = "requires docker"]
async fn refresh_checks_every_movie_in_windows_when_last_sync_is_older_than_two_weeks() {
    let shared = Arc::new(database().await);
    let db: &DbConn = &shared;
    let f = fakes(&shared);
    let user = seed::user(db, 1).await;
    seed::movie(db, &user, 603, "The Matrix").await;
    seed::series(db, &user, 70523, "Dark").await;
    for synced_at in [Utc.with_ymd_and_hms(2023, 11, 1, 0, 0, 0), Utc.with_ymd_and_hms(2023, 12, 1, 0, 0, 0)] {
        sync_state::ActiveModel { id: NotSet, synced_at: Set(synced_at.unwrap().fixed_offset()) }
            .insert(db).await.expect("inserting sync state");
    }
    let clock = fixed_clock(2024, 1, 5, 12, 0, 0);
    let mut tmdb = MockTmdb::new();
    tmdb.expect_movie_property_changes().withf(|id, _, _| *id == 603).times(3).returning(|_, _, _| Ok(property_changes(&[])));
    tmdb.expect_series_property_changes().withf(|id, _, _| *id == 70523).times(3).returning(|_, _, _| Ok(property_changes(&[])));
    tmdb.expect_season_property_changes().never();

    let result = refresh(db, &tmdb, &f.store, &f.jobs, &clock, &f.random).await.expect("refresh should succeed");

    assert_eq!((result.updates, result.errors), (6, 0));
    let latest = SyncState::find().order_by_desc(sync_state::Column::SyncedAt).one(db).await.expect("querying sync state").expect("a sync state");
    assert_eq!(latest.synced_at, Utc.with_ymd_and_hms(2024, 1, 5, 12, 0, 0).unwrap().fixed_offset());
}

/// Three logs rated 2, 5 and 9 spread to 0, 5 and 10 (see the unit tests of `linearize_ratings`).
async fn three_rated_logs(db: &DbConn) -> (views::users::CurrentUser, i32) {
    let user = seed::user(db, 1).await;
    let movie = seed::movie(db, &user, 603, "The Matrix").await;
    let source = seed::source(db, movie.id, "Jellyfin").await;
    for stars in [2.0, 5.0, 9.0] {
        seed::log(db, movie.id, source.id, Some(stars), true).await;
    }
    (user, movie.id)
}

async fn log_ratings(db: &DbConn) -> Vec<(Option<f32>, Option<f32>)> {
    Logs::find().order_by_asc(logs::Column::Id).all(db).await.expect("querying logs")
        .into_iter().map(|l| (l.stars, l.original_stars)).collect()
}

#[tokio::test]
#[ignore = "requires docker"]
async fn linearize_ratings_updates_real_rows() {
    let db = database().await;
    let (user, media_id) = three_rated_logs(&db).await;

    let result = linearize_ratings(&db, Some(user.uuid), false).await.expect("linearization should succeed");

    assert_eq!((result.users_processed, result.logs_updated, result.media_recomputed), (1, 3, 1));
    assert_eq!(log_ratings(&db).await, vec![(Some(0.0), Some(2.0)), (Some(5.0), Some(5.0)), (Some(10.0), Some(9.0))]);
    let media = Media::find_by_id(media_id).one(&db).await.expect("querying media").expect("media row");
    assert_eq!(media.stars, Some(5.0));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn linearize_ratings_leaves_rows_unchanged_when_dry_run() {
    let db = database().await;
    let (user, media_id) = three_rated_logs(&db).await;

    let result = linearize_ratings(&db, Some(user.uuid), true).await.expect("linearization should succeed");

    assert_eq!((result.dry_run, result.logs_updated), (true, 3));
    assert_eq!(log_ratings(&db).await, vec![(Some(2.0), None), (Some(5.0), None), (Some(9.0), None)]);
    let media = Media::find_by_id(media_id).one(&db).await.expect("querying media").expect("media row");
    assert_eq!(media.stars, None);
}
