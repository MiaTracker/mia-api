use sea_orm::DbConn;
use views::logs::LogCreate;
use views::logset::LogsetCreate;
use views::media::MediaType;
use views::users::CurrentUser;
use crate::infrastructure::{Clock, ImageStore, Random, SrvErr, Tmdb};
use crate::jobs::Jobs;

#[allow(clippy::too_many_arguments)]
pub async fn create(logset: &LogsetCreate, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, store: &dyn ImageStore, jobs: &dyn Jobs, clock: &dyn Clock, random: &dyn Random) -> Result<(), SrvErr> {
    if logset.media_id.is_some() && logset.external_id.is_some() {
        return Err(SrvErr::BadRequest("Client sent both media_id and external_id. Only one of them must be set.".to_string()))
    }
    if logset.media_id.is_none() && logset.external_id.is_none() {
        return Err(SrvErr::BadRequest("Client sent neither media_id nor external_id. One of them must be set.".to_string()))
    }

    if logset.source.is_some() && logset.new_source.is_some() {
        return Err(SrvErr::BadRequest("Client sent both source and new_source. Only one of them must be set.".to_string()))
    }
    if logset.source.is_none() && logset.new_source.is_none() {
        return Err(SrvErr::BadRequest("Client sent neither source nor new_source. One of them must be set.".to_string()))
    }

    let media_id;
    if let Some(external) = logset.external_id {
        if logset.media_type == MediaType::Movie {
            let (_, id) = crate::services::movies::create(external, user, db, tmdb, store, jobs, clock, random).await?;
            media_id = id
        }
        else {
            let (_, id) = crate::services::series::create(external, user, db, tmdb, store, jobs, clock, random).await?;
            media_id = id
        }
    } else { media_id = logset.media_id.unwrap() }

    let source_name;
    if let Some(source) = &logset.new_source {
        crate::services::sources::create(media_id, &source, logset.media_type, user, db).await?;
        source_name = source.name.clone();
    } else { source_name = logset.source.clone().unwrap() }

    let log = LogCreate {
        date: logset.date,
        stars: logset.stars,
        completed: logset.completed,
        comment: logset.comment.clone(),
        source: source_name,
    };

    crate::services::logs::create(media_id, &log, logset.media_type, user, db).await?;

    if logset.remove_from_watchlist == Some(true) {
        crate::services::watchlist::remove(media_id, user, db).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use entities::{media, sources};
    use integrations::tmdb::views::{MovieDetails as TmdbMovieDetails, MovieTitles, SeriesDetails as TmdbSeriesDetails, SeriesTitles};
    use sea_orm::Value;
    use views::sources::{SourceCreate, SourceType};
    use super::*;
    use crate::test_support;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, deleted};
    use crate::test_support::fixtures;
    use crate::test_support::{date, fixed_clock, InMemoryImageStore, MockTmdb, RecordingJobs, SeqRandom};

    /// A logset for existing movie 10 and its source "Torrent A". Tests override fields with
    /// struct-update syntax.
    fn logset() -> LogsetCreate {
        LogsetCreate {
            media_id: Some(10),
            external_id: None,
            media_type: MediaType::Movie,
            source: Some("Torrent A".to_string()),
            new_source: None,
            date: date(2026, 3, 1),
            stars: Some(8.0),
            completed: true,
            comment: Some("Great".to_string()),
            remove_from_watchlist: None,
        }
    }

    fn new_source() -> SourceCreate {
        SourceCreate { name: "Torrent A".to_string(), url: "https://example.test/new".to_string(), r#type: SourceType::Torrent }
    }

    /// Runs `create` as user 1 with a fixed clock and sequential randomness.
    async fn run_create(logset: &LogsetCreate, db: &DbConn, tmdb: &MockTmdb) -> Result<(), SrvErr> {
        create(logset, &fixtures::user(), db, tmdb, &InMemoryImageStore::new(), &RecordingJobs::new(), &fixed_clock(2026, 3, 4, 12, 0, 0), &SeqRandom::new()).await
    }

    /// Asserts `result` is a bad request with `message`.
    fn assert_bad_request(result: Result<(), SrvErr>, message: &str) {
        match result {
            Err(SrvErr::BadRequest(actual)) => assert_eq!(actual, message),
            Err(_) => panic!("expected a bad request, got another error"),
            Ok(()) => panic!("expected a bad request, got Ok"),
        }
    }

    #[tokio::test]
    async fn create_creates_media_when_missing_then_adds_log() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_movie_details().withf(|id| *id == 603).times(1).returning(|id| Ok(TmdbMovieDetails {
            backdrop_path: None, poster_path: None, ..test_support::tmdb::movie_details(id)
        }));
        tmdb.expect_movie_alternative_titles().times(1).returning(|id| Ok(MovieTitles { id, titles: Vec::new() }));
        let db = test_support::db::mock()
            // movie: not added yet, then media, movie and title rows
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::movie_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Movie 603", true)]])
            // new source: media lookup, name check, source row
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            // log: media lookup, source lookup, log row, rating, media row
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::log_model(1, 10, 3)]])
            .append_query_results([[fixtures::rating_row(8.0, 1)]])
            .append_query_results([[media::Model { stars: Some(8.0), ..fixtures::movie_media_model(10, 1) }]])
            .into_connection();
        let logset = LogsetCreate { media_id: None, external_id: Some(603), source: None, new_source: Some(new_source()), ..logset() };

        let result = run_create(&logset, &db, &tmdb).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "movies""#, &[Value::from(10)]);
        assert_wrote(&log, r#"INSERT INTO "sources""#, &[Value::from(10), Value::from("Torrent A")]);
        assert_wrote(&log, r#"INSERT INTO "logs""#, &[Value::from(10), Value::from(3), Value::from(date(2026, 3, 1)), Value::from(8.0f32), Value::from("Great")]);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "watchlist""#);
    }

    #[tokio::test]
    async fn create_creates_series_when_media_type_is_series() {
        test_support::init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_series_details().withf(|id| *id == 70).times(1).returning(|id| Ok(TmdbSeriesDetails {
            backdrop_path: None, poster_path: None, ..test_support::tmdb::series_details(id)
        }));
        tmdb.expect_series_alternative_titles().times(1).returning(|id| Ok(SeriesTitles { id, results: Vec::new() }));
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::series_model(10)]])
            .append_query_results([[fixtures::title_model(1, 10, "Series 70", true)]])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::count_row(0)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::log_model(1, 10, 3)]])
            .append_query_results([[fixtures::rating_row(8.0, 1)]])
            .append_query_results([[fixtures::series_media_model(10, 1)]])
            .into_connection();
        let logset = LogsetCreate { media_id: None, external_id: Some(70), media_type: MediaType::Series, source: None, new_source: Some(new_source()), ..logset() };

        let result = run_create(&logset, &db, &tmdb).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "series""#, &[Value::from(10)]);
        assert_wrote(&log, r#"INSERT INTO "logs""#, &[Value::from(10), Value::from(3)]);
    }

    #[tokio::test]
    async fn create_uses_existing_media() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::log_model(1, 10, 3)]])
            .append_query_results([[fixtures::rating_row(8.0, 1)]])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .into_connection();

        // The TMDB mock has no expectations: nothing is looked up.
        let result = run_create(&logset(), &db, &MockTmdb::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "logs""#, &[Value::from(10), Value::from(3), Value::from(8.0f32), Value::from("Great")]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "media""#);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "sources""#);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "watchlist""#);
    }

    #[tokio::test]
    async fn create_removes_media_from_watchlist_when_requested() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::log_model(1, 10, 3)]])
            .append_query_results([[fixtures::rating_row(8.0, 1)]])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::watchlist_model(10)]])
            .append_exec_results([deleted()])
            .into_connection();
        let logset = LogsetCreate { remove_from_watchlist: Some(true), ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "logs""#, &[Value::from(10)]);
        assert_wrote(&log, r#"DELETE FROM "watchlist""#, &[Value::from(10)]);
    }

    #[tokio::test]
    async fn create_keeps_media_on_watchlist_when_remove_from_watchlist_false() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::source_model(3, 10, "Torrent A")]])
            .append_query_results([[fixtures::log_model(1, 10, 3)]])
            .append_query_results([[fixtures::rating_row(8.0, 1)]])
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .into_connection();
        let logset = LogsetCreate { remove_from_watchlist: Some(false), ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert!(result.is_ok());
        assert_no_statement_starting_with(&test_support::db::log(db), r#"DELETE FROM "watchlist""#);
    }

    #[tokio::test]
    async fn create_returns_err_and_adds_no_log_when_new_source_name_already_exists() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([[fixtures::count_row(1)]])
            .into_connection();
        let logset = LogsetCreate { source: None, new_source: Some(new_source()), ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert!(matches!(result, Err(SrvErr::RuleViolation(_))));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn create_returns_not_found_when_existing_media_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<media::Model>::new()])
            .into_connection();

        let result = run_create(&logset(), &db, &MockTmdb::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn create_returns_not_found_when_source_missing() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::movie_media_model(10, 1)]])
            .append_query_results([Vec::<sources::Model>::new()])
            .into_connection();

        let result = run_create(&logset(), &db, &MockTmdb::new()).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        let log = test_support::db::log(db);
        assert_statement_count(&log, 2);
        assert_no_statement_starting_with(&log, "INSERT");
    }

    #[tokio::test]
    async fn create_returns_bad_request_when_both_media_id_and_external_id_given() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let logset = LogsetCreate { external_id: Some(603), ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert_bad_request(result, "Client sent both media_id and external_id. Only one of them must be set.");
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn create_returns_bad_request_when_neither_media_id_nor_external_id_given() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let logset = LogsetCreate { media_id: None, ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert_bad_request(result, "Client sent neither media_id nor external_id. One of them must be set.");
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn create_returns_bad_request_when_both_source_and_new_source_given() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let logset = LogsetCreate { new_source: Some(new_source()), ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert_bad_request(result, "Client sent both source and new_source. Only one of them must be set.");
        assert_statement_count(&test_support::db::log(db), 0);
    }

    #[tokio::test]
    async fn create_returns_bad_request_when_neither_source_nor_new_source_given() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let logset = LogsetCreate { source: None, ..logset() };

        let result = run_create(&logset, &db, &MockTmdb::new()).await;

        assert_bad_request(result, "Client sent neither source nor new_source. One of them must be set.");
        assert_statement_count(&test_support::db::log(db), 0);
    }
}
