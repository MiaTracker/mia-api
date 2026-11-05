use chrono::Duration;
use futures::StreamExt;
use sea_orm::{ActiveModelTrait, ColumnTrait, Database, DbConn, EntityTrait, FromQueryResult, NotSet, Order, QueryFilter, QueryOrder, QuerySelect};
use sea_orm::ActiveValue::Set;
use entities::prelude::{Media, SyncState};
use entities::sea_orm_active_enums::MediaType;
use entities::{media, sync_state};
use log::{error, info};
use infrastructure::config;
use views::refresh::RefreshResult;
use std::sync::Arc;
use crate::infrastructure::{Clock, FsImageStore, ImageStore, openrouter_llm, OsRandom, Random, SrvErr, SystemClock, Tmdb, TmdbClient};
use crate::chat::registry::ExchangeRegistry;
use crate::jobs::{JobDeps, Jobs, TokioJobs};
use crate::{movies, series};

pub async fn run_refresh() {
    let tmdb: Arc<dyn Tmdb> = Arc::new(TmdbClient::new(config().tmdb.authorization_token.clone()));
    crate::infrastructure::initialize(tmdb.as_ref()).await;

    let db_url = config().db.connection_url.clone();
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    let conn = Arc::new(conn);
    let store: Arc<dyn ImageStore> = Arc::new(FsImageStore::new(config().images.store_path.clone()));
    let llm = openrouter_llm(&config().openrouter);
    let jobs = TokioJobs::new(JobDeps {
        db: conn.clone(), store: store.clone(), llm, title_llm: None, registry: Arc::new(ExchangeRegistry::new()), tmdb: tmdb.clone(),
        clock: Arc::new(SystemClock),
    });

    let refreshed = refresh(&conn, tmdb.as_ref(), store.as_ref(), &jobs, &SystemClock, &OsRandom).await;
    // The image sizes are saved by background jobs, which exiting would cancel.
    jobs.finish().await;

    match refreshed {
        Ok(result) => {
            println!("Refresh complete. Updated: {}, Errors: {}", result.updates, result.errors);
        }
        Err(err) => {
            eprintln!("Refresh failed: {}", err);
            std::process::exit(1);
        }
    }
}

pub async fn refresh(db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, clock: &dyn Clock, random: &dyn Random) -> Result<RefreshResult, SrvErr> {
    let last_state = SyncState::find().order_by(sync_state::Column::SyncedAt, Order::Desc).one(db).await?;

    let now = clock.now();

    let start_date = match last_state {
        None => {
            let m = Media::find().order_by(media::Column::DateAdded, Order::Asc).one(db).await?;
            m.map(|m| m.date_added).unwrap_or(now.date_naive())
        }
        Some(s) => { s.synced_at.date_naive() }
    };
    let end_date = now.date_naive();

    async fn fetch_all_tmdb_ids(media_type: MediaType, db: &DbConn) -> Result<Vec<i32>, SrvErr> {
        #[derive(FromQueryResult)]
        struct IdEntity { pub tmdb_id: Option<i32> }

        Ok(Media::find()
            .select_only().columns([media::Column::TmdbId])
            .filter(media::Column::Type.eq(media_type))
            .into_model::<IdEntity>()
            .all(db)
            .await?
            .into_iter()
            .filter_map(|e| e.tmdb_id)
            .collect())
    }

    let within_window = end_date - start_date < Duration::days(14);
    let (movie_tmdb_ids, series_tmdb_ids) = futures::try_join!(
        async {
            if within_window {
                match tmdb.movie_changed(start_date, end_date).await {
                    Ok(ids) => Ok(ids),
                    Err(e) => {
                        info!("Failed to fetch changed movie IDs from TMDB: {} Falling back to full update", e);
                        fetch_all_tmdb_ids(MediaType::Movie, db).await
                    }
                }
            } else {
                fetch_all_tmdb_ids(MediaType::Movie, db).await
            }
        },
        async {
            if within_window {
                match tmdb.series_changed(start_date, end_date).await {
                    Ok(ids) => Ok(ids),
                    Err(e) => {
                        info!("Failed to fetch changed series IDs from TMDB: {} Falling back to full update", e);
                        fetch_all_tmdb_ids(MediaType::Series, db).await
                    }
                }
            } else {
                fetch_all_tmdb_ids(MediaType::Series, db).await
            }
        }
    )?;

    let intervals = build_intervals(start_date, now.date_naive());

    let mut updates = 0usize;
    let mut errors = 0usize;

    for (start_date, end_date) in intervals {
        let mut futures: Vec<std::pin::Pin<Box<dyn Future<Output = (usize, usize)> + Send + '_>>> = Vec::new();

        if !movie_tmdb_ids.is_empty() {
            let movie_media: Vec<media::Model> = media::Entity::find()
                .filter(media::Column::TmdbId.is_in(movie_tmdb_ids.clone()))
                .filter(media::Column::Type.eq(MediaType::Movie))
                .all(db)
                .await?;

            for med in movie_media {
                futures.push(Box::pin(async move {
                    let tmdb_id = match med.tmdb_id {
                        Some(id) => id,
                        None => return (0, 0),
                    };
                    let changes = match tmdb.movie_property_changes(tmdb_id, start_date, end_date).await {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to fetch changes for movie tmdb_id={}: {}", tmdb_id, e);
                            return (0, 1);
                        }
                    };
                    match movies::apply_changes(&med, &changes, db, tmdb, store, jobs, random).await {
                        Ok(_) => (1, 0),
                        Err(e) => {
                            error!("Failed to apply changes for movie media_id={}: {}", med.id, e);
                            (0, 1)
                        }
                    }
                }));
            }
        }

        if !series_tmdb_ids.is_empty() {
            let series_media: Vec<media::Model> = media::Entity::find()
                .filter(media::Column::TmdbId.is_in(series_tmdb_ids.clone()))
                .filter(media::Column::Type.eq(MediaType::Series))
                .all(db)
                .await?;

            for med in series_media {
                futures.push(Box::pin(async move {
                    let tmdb_id = match med.tmdb_id {
                        Some(id) => id,
                        None => return (0, 0),
                    };
                    let changes = match tmdb.series_property_changes(tmdb_id, start_date, end_date).await {
                        Ok(c) => c,
                        Err(e) => {
                            error!("Failed to fetch changes for series tmdb_id={}: {}", tmdb_id, e);
                            return (0, 1);
                        }
                    };
                    match series::apply_changes(&med, &changes, start_date, end_date, db, tmdb, store, jobs, random).await {
                        Ok(_) => (1, 0),
                        Err(e) => {
                            error!("Failed to apply changes for series media_id={}: {}", med.id, e);
                            (0, 1)
                        }
                    }
                }));
            }
        }

        let results: Vec<(usize, usize)> = futures::stream::iter(futures)
            .buffer_unordered(15)
            .collect()
            .await;
        for (u, e) in results {
            updates += u;
            errors += e;
        }
    }

    // Update last_synced_at
    let state_am = sync_state::ActiveModel {
        id: NotSet,
        synced_at: Set(now.fixed_offset()),
    };
    state_am.insert(db).await?;

    Ok(RefreshResult { updates, errors })
}

/// Split [start, end] into non-overlapping 14-day windows.
fn build_intervals(
    start: chrono::NaiveDate,
    end: chrono::NaiveDate,
) -> Vec<(chrono::NaiveDate, chrono::NaiveDate)> {
    let mut intervals = Vec::new();
    let mut current = start;
    while current < end {
        let next = (current + Duration::days(14)).min(end);
        intervals.push((current, next));
        current = next;
    }
    intervals
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use chrono::NaiveDate;
    use integrations::tmdb::views::PropertyChanges;
    use sea_orm::{DbErr, Value};
    use super::*;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote};
    use crate::test_support::fixtures;
    use crate::test_support::{date, fixed_clock, FixedClock, InMemoryImageStore, MockTmdb, RecordingJobs, SeqRandom};

    // ---------------------------------------------------------------- helpers

    async fn run_refresh(db: &DbConn, tmdb: &MockTmdb, clock: &FixedClock) -> Result<RefreshResult, SrvErr> {
        refresh(db, tmdb, &InMemoryImageStore::new(), &RecordingJobs::new(), clock, &SeqRandom::new()).await
    }

    /// Expects exactly one `movie_changed(start, end)` and one `series_changed(start, end)`.
    fn expect_changed(tmdb: &mut MockTmdb, start: NaiveDate, end: NaiveDate, movie_ids: Vec<i32>, series_ids: Vec<i32>) {
        tmdb.expect_movie_changed()
            .withf(move |s, e| *s == start && *e == end)
            .times(1)
            .returning(move |_, _| Ok(movie_ids.clone()));
        tmdb.expect_series_changed()
            .withf(move |s, e| *s == start && *e == end)
            .times(1)
            .returning(move |_, _| Ok(series_ids.clone()));
    }

    /// Expects exactly one `movie_property_changes(tmdb_id, start, end)`, answering with `changes`.
    fn expect_movie_changes(tmdb: &mut MockTmdb, tmdb_id: i32, start: NaiveDate, end: NaiveDate, changes: PropertyChanges) {
        let mut changes = Some(changes);
        tmdb.expect_movie_property_changes()
            .withf(move |id, s, e| *id == tmdb_id && *s == start && *e == end)
            .times(1)
            .returning(move |_, _, _| Ok(changes.take().expect("answered once")));
    }

    fn tracked_movie(id: i32, tmdb_id: i32) -> media::Model {
        media::Model { tmdb_id: Some(tmdb_id), ..fixtures::movie_media_model(id, 1) }
    }

    fn tracked_series(id: i32, tmdb_id: i32) -> media::Model {
        media::Model { tmdb_id: Some(tmdb_id), ..fixtures::series_media_model(id, 1) }
    }

    fn runtime_change() -> PropertyChanges {
        test_support::tmdb::property_changes(&[("runtime", serde_json::json!(120))])
    }

    /// The row of the "all tracked TMDB ids" query.
    fn tmdb_id_row(tmdb_id: i32) -> BTreeMap<&'static str, Value> {
        BTreeMap::from([("tmdb_id", Value::from(tmdb_id))])
    }

    /// A `sync_state` row for the given clock; the insert at the end of `refresh` returns one.
    fn synced_at(id: i32, clock: &FixedClock) -> entities::sync_state::Model {
        fixtures::sync_state_model(id, clock.now())
    }

    // ---------------------------------------------------------------- refresh

    #[tokio::test]
    async fn refresh_uses_earliest_date_added_when_never_synced() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 5), date(2024, 3, 10), vec![], vec![]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::sync_state::Model>::new()])
            .append_query_results([[media::Model { date_added: date(2024, 3, 5), ..fixtures::movie_media_model(1, 1) }]])
            .append_query_results([[synced_at(1, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (0, 0));
        assert_statement_count(&test_support::db::log(db), 3);
    }

    #[tokio::test]
    async fn refresh_uses_today_when_never_synced_and_no_media() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 10), date(2024, 3, 10), vec![], vec![]);
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::sync_state::Model>::new()])
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[synced_at(1, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (0, 0));
        assert_statement_count(&test_support::db::log(db), 3);
    }

    #[tokio::test]
    async fn refresh_uses_last_sync_date_when_synced() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 1), date(2024, 3, 10), vec![], vec![]);
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 1, 23, 30, 0))]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (0, 0));
        // Only the sync_state lookup and insert: the media table is not consulted for the start date.
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn refresh_applies_changes_only_for_tracked_tmdb_ids() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        // TMDB reports changes to 100 and 200, but only 100 is tracked.
        expect_changed(&mut tmdb, date(2024, 3, 8), date(2024, 3, 10), vec![100, 200], vec![]);
        expect_movie_changes(&mut tmdb, 100, date(2024, 3, 8), date(2024, 3, 10), runtime_change());
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[tracked_movie(7, 100)]])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<entities::movie_locks::Model>::new()])
            .append_query_results([[fixtures::movie_model(7)]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (1, 0));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"SELECT "media"."id""#, &[Value::from(100), Value::from(200)]);
        assert_wrote(&log, r#"UPDATE "movies""#, &[Value::from(120), Value::from(7)]);
    }

    #[tokio::test]
    async fn refresh_applies_series_changes_for_tracked_series() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 8), date(2024, 3, 10), vec![], vec![70]);
        tmdb.expect_series_property_changes()
            .withf(|id, s, e| *id == 70 && *s == date(2024, 3, 8) && *e == date(2024, 3, 10))
            .times(1)
            .returning(|_, _, _| Ok(test_support::tmdb::property_changes(&[("status", serde_json::json!("Ended"))])));
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[tracked_series(8, 70)]])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<entities::series_locks::Model>::new()])
            .append_query_results([[fixtures::series_model(8)]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (1, 0));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "series""#, &[Value::from("Ended"), Value::from(8)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "movies""#);
    }

    #[tokio::test]
    async fn refresh_skips_media_without_tmdb_id_when_returned_for_tracked_ids() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 8), date(2024, 3, 10), vec![100], vec![]);
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[fixtures::movie_media_model(7, 1)]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (0, 0));
        assert_statement_count(&test_support::db::log(db), 3);
    }

    #[tokio::test]
    async fn refresh_counts_errors_without_aborting() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 8), date(2024, 3, 10), vec![100, 200], vec![]);
        tmdb.expect_movie_property_changes()
            .withf(|id, _, _| *id == 100)
            .times(1)
            .returning(|_, _, _| Err(test_support::tmdb::error("boom")));
        expect_movie_changes(&mut tmdb, 200, date(2024, 3, 8), date(2024, 3, 10), runtime_change());
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[tracked_movie(7, 100), tracked_movie(8, 200)]])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<entities::movie_locks::Model>::new()])
            .append_query_results([[fixtures::movie_model(8)]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (1, 1));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "movies""#, &[Value::from(120), Value::from(8)]);
        assert_wrote(&log, r#"INSERT INTO "sync_state""#, &[]);
    }

    #[tokio::test]
    async fn refresh_counts_error_when_applying_changes_fails() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 8), date(2024, 3, 10), vec![100], vec![]);
        expect_movie_changes(&mut tmdb, 100, date(2024, 3, 8), date(2024, 3, 10), runtime_change());
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[tracked_movie(7, 100)]])
            .append_query_errors([DbErr::Custom("locks unavailable".into())])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (0, 1));
        assert_wrote(&test_support::db::log(db), r#"INSERT INTO "sync_state""#, &[]);
    }

    #[tokio::test]
    async fn refresh_records_sync_state_at_now() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 34, 56);
        let mut tmdb = MockTmdb::new();
        expect_changed(&mut tmdb, date(2024, 3, 8), date(2024, 3, 10), vec![], vec![]);
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "sync_state""#, &[Value::from(clock.now().fixed_offset())]);
    }

    #[tokio::test]
    async fn refresh_tracks_all_ids_when_changed_call_fails() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_changed().times(1).returning(|_, _| Err(test_support::tmdb::error("TMDB is down")));
        tmdb.expect_series_changed().times(1).returning(|_, _| Ok(vec![]));
        expect_movie_changes(&mut tmdb, 100, date(2024, 3, 8), date(2024, 3, 10), runtime_change());
        let db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 3, 8, 0, 0, 0))]])
            .append_query_results([[tmdb_id_row(100)]])
            .append_query_results([[tracked_movie(7, 100)]])
            .append_query_results([Vec::<entities::media_locks::Model>::new()])
            .append_query_results([Vec::<entities::movie_locks::Model>::new()])
            .append_query_results([[fixtures::movie_model(7)]])
            .append_query_results([[synced_at(2, &clock)]])
            .into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (1, 0));
        assert_wrote(&test_support::db::log(db), r#"UPDATE "movies""#, &[Value::from(120), Value::from(7)]);
    }

    #[tokio::test]
    async fn refresh_walks_each_14_day_interval_when_last_sync_is_14_days_or_older() {
        test_support::init();
        let clock = fixed_clock(2024, 3, 10, 12, 0, 0);
        // No changed-IDs lookup at all: MockTmdb panics on movie_changed / series_changed.
        let mut tmdb = MockTmdb::new();
        let no_changes = || test_support::tmdb::property_changes(&[]);
        expect_movie_changes(&mut tmdb, 100, date(2024, 2, 1), date(2024, 2, 15), no_changes());
        expect_movie_changes(&mut tmdb, 100, date(2024, 2, 15), date(2024, 2, 29), no_changes());
        expect_movie_changes(&mut tmdb, 100, date(2024, 2, 29), date(2024, 3, 10), no_changes());
        let mut db = test_support::db::mock()
            .append_query_results([[synced_at(1, &fixed_clock(2024, 2, 1, 0, 0, 0))]])
            .append_query_results([[tmdb_id_row(100)]])
            .append_query_results([Vec::<BTreeMap<&str, Value>>::new()]);
        for _ in 0..3 {
            db = db
                .append_query_results([[tracked_movie(7, 100)]])
                .append_query_results([Vec::<entities::media_locks::Model>::new()])
                .append_query_results([Vec::<entities::movie_locks::Model>::new()]);
        }
        let db = db.append_query_results([[synced_at(2, &clock)]]).into_connection();

        let result = run_refresh(&db, &tmdb, &clock).await.ok().expect("refresh succeeds");

        assert_eq!((result.updates, result.errors), (3, 0));
        // sync_state lookup, both tracked-id lookups, 3 intervals of (media, media locks, movie locks), insert.
        assert_statement_count(&test_support::db::log(db), 1 + 2 + 3 * 3 + 1);
    }

    // ---------------------------------------------------------------- build_intervals

    #[test]
    fn build_intervals_returns_nothing_when_start_is_not_before_end() {
        assert!(build_intervals(date(2024, 3, 10), date(2024, 3, 10)).is_empty());
        assert!(build_intervals(date(2024, 3, 11), date(2024, 3, 10)).is_empty());
    }

    #[test]
    fn build_intervals_returns_one_interval_when_range_is_shorter_than_14_days() {
        assert_eq!(build_intervals(date(2024, 3, 1), date(2024, 3, 6)), vec![(date(2024, 3, 1), date(2024, 3, 6))]);
    }

    #[test]
    fn build_intervals_returns_one_interval_when_range_is_exactly_14_days() {
        assert_eq!(build_intervals(date(2024, 3, 1), date(2024, 3, 15)), vec![(date(2024, 3, 1), date(2024, 3, 15))]);
    }

    #[test]
    fn build_intervals_splits_into_adjacent_14_day_windows_when_range_is_longer() {
        let intervals = build_intervals(date(2024, 2, 1), date(2024, 3, 10));

        assert_eq!(intervals, vec![
            (date(2024, 2, 1), date(2024, 2, 15)),
            (date(2024, 2, 15), date(2024, 2, 29)),
            (date(2024, 2, 29), date(2024, 3, 10)),
        ]);
    }
}
