//! The `tmdb_search` tool: searches TMDB and marks the hits already in the user's library.

use entities::media;
use entities::prelude::Media;
use integrations::tmdb::views::{MultiMovieResult, MultiTvResult};
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter};
use serde_json::{json, Value};
use views::conversations::ToolCallView;
use views::media::MediaType;
use views::users::CurrentUser;

use crate::chat::tools::{failed, ToolOutcome};
use crate::infrastructure::{SrvErr, Tmdb};
use crate::media::{search_tmdb, TmdbHit};

const NAME: &str = "tmdb_search";

/// What the model is told about the tool.
pub const DESCRIPTION: &str = "Searches TMDB for movies and series by title, optionally only one type. Returns one page \
of up to 20 results with TMDB id, type, title, original title, overview, release or first air year, original language \
and TMDB rating (null when nobody voted). A result already in the user's library has its `mia_id`, which can be passed to mia_details; for the \
others `mia_id` is null.";

/// Searches TMDB for `input["query"]` (only `input["type"]` when given) and marks each hit that is
/// in `user`'s library with its `mia_id`, with one library query whatever the number of hits.
pub async fn run(input: &Value, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<ToolOutcome, SrvErr> {
    let Some(query) = input["query"].as_str() else {
        return Ok(failed(NAME, input, "input must have a string 'query'"));
    };
    let media_type = match input.get("type") {
        None | Some(Value::Null) => None,
        Some(Value::String(name)) if name == "movie" => Some(MediaType::Movie),
        Some(Value::String(name)) if name == "series" => Some(MediaType::Series),
        Some(_) => return Ok(failed(NAME, input, "type must be \"movie\" or \"series\""))
    };
    let hits = match search_tmdb(query.to_string(), media_type, tmdb).await {
        Ok(hits) => hits,
        Err(error) => return Ok(failed(NAME, input, &format!("TMDB search failed: {error}")))
    };
    let library = library_rows(&hits, user, db).await?;

    let results: Vec<Value> = hits.iter().map(|hit| {
        let mia_id = library.iter().find(|row| row.tmdb_id == Some(hit.tmdb_id()) && row.r#type == hit.media_type().into()).map(|row| row.id);
        match hit {
            TmdbHit::Movie(movie) => movie_result(movie, mia_id),
            TmdbHit::Tv(tv) => tv_result(tv, mia_id)
        }
    }).collect();

    let display = ToolCallView {
        name: NAME.into(),
        query: Some(query.to_string()),
        media_type: media_type.map(|media_type| type_name(media_type).to_string()),
        count: Some(results.len() as u32),
        titles: None,
        failed: false
    };
    Ok(ToolOutcome { content: json!({ "results": results }).to_string(), is_error: false, display })
}

/// The media in `user`'s library with the TMDB id of any of `hits`, of either type: one query,
/// none when there are no hits.
async fn library_rows(hits: &[TmdbHit], user: &CurrentUser, db: &DbConn) -> Result<Vec<media::Model>, SrvErr> {
    if hits.is_empty() {
        return Ok(Vec::new());
    }
    let tmdb_ids: Vec<i32> = hits.iter().map(TmdbHit::tmdb_id).collect();
    Ok(Media::find().filter(media::Column::UserId.eq(user.id)).filter(media::Column::TmdbId.is_in(tmdb_ids)).all(db).await?)
}

fn movie_result(movie: &MultiMovieResult, mia_id: Option<i32>) -> Value {
    let rating = rating(movie.vote_average, movie.vote_count);
    result(MediaType::Movie, movie.id, &movie.title, &movie.original_title, &movie.overview, &movie.release_date, &movie.original_language, rating, mia_id)
}

fn tv_result(tv: &MultiTvResult, mia_id: Option<i32>) -> Value {
    let rating = rating(tv.vote_average, tv.vote_count);
    result(MediaType::Series, tv.id, &tv.name, &tv.original_name, &tv.overview, &tv.first_air_date, &tv.original_language, rating, mia_id)
}

/// The TMDB rating to show: `None` when nobody voted, since TMDB then reports an average of 0.
fn rating(vote_average: Option<f32>, vote_count: Option<i32>) -> Option<f64> {
    if vote_count.unwrap_or(0) == 0 {
        return None;
    }
    vote_average.and_then(|rating| rating.to_string().parse::<f64>().ok())
}

#[allow(clippy::too_many_arguments)]
fn result(media_type: MediaType, tmdb_id: i32, title: &Option<String>, original_title: &Option<String>, overview: &Option<String>, date: &Option<String>, original_language: &Option<String>, tmdb_rating: Option<f64>, mia_id: Option<i32>) -> Value {
    json!({
        "tmdb_id": tmdb_id,
        "type": type_name(media_type),
        "title": title,
        "original_title": original_title,
        "overview": overview,
        "year": date.as_deref().and_then(year),
        "original_language": original_language,
        "tmdb_rating": tmdb_rating,
        "mia_id": mia_id
    })
}

/// The name the model sees for `media_type`, as in the tool's `type` input.
fn type_name(media_type: MediaType) -> &'static str {
    match media_type {
        MediaType::Movie => "movie",
        MediaType::Series => "series"
    }
}

/// The year a TMDB date (`YYYY-MM-DD`) starts with, when it has one.
fn year(date: &str) -> Option<i32> {
    date.get(..4)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use entities::media;
    use entities::sea_orm_active_enums::MediaType;
    use serde_json::json;
    use views::conversations::ToolCallView;

    use integrations::tmdb::views::MultiResult;
    use crate::test_support::fixtures::{media_model, user, user_model};
    use crate::test_support::tmdb::{error, full_multi_movie, full_multi_tv, multi_results};
    use crate::test_support::{db, init, MockTmdb};

    fn tmdb_returning(results: Vec<integrations::tmdb::views::MultiResult>) -> MockTmdb {
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().withf(|query| query == "alien").times(1).returning(move |_| Ok(multi_results(results.clone())));
        tmdb
    }

    fn library_row(id: i32, tmdb_id: i32, media_type: MediaType) -> media::Model {
        media::Model { tmdb_id: Some(tmdb_id), ..media_model(id, 1, media_type) }
    }

    fn content(outcome: &ToolOutcome) -> Value {
        serde_json::from_str(&outcome.content).expect("content is JSON")
    }

    #[tokio::test]
    async fn run_marks_hits_already_in_library_with_mia_id() {
        init();
        let tmdb = tmdb_returning(vec![full_multi_movie(348), full_multi_tv(1399)]);
        let conn = db::mock().append_query_results([vec![library_row(5, 348, MediaType::Movie)]]).into_connection();

        let outcome = run(&json!({ "query": "alien" }), &user(), &conn, &tmdb).await.ok().expect("run");

        assert!(!outcome.is_error);
        let results = content(&outcome)["results"].clone();
        assert_eq!(results[0], json!({
            "tmdb_id": 348, "type": "movie", "title": "Movie 348", "original_title": "Original movie 348",
            "overview": "Overview of movie 348", "year": 1979, "original_language": "en", "tmdb_rating": 8.2, "mia_id": 5
        }));
        assert_eq!(results[1], json!({
            "tmdb_id": 1399, "type": "series", "title": "Series 1399", "original_title": "Original series 1399",
            "overview": "Overview of series 1399", "year": 2011, "original_language": "en", "tmdb_rating": 8.4, "mia_id": null
        }));
        assert_eq!(outcome.display, ToolCallView { name: "tmdb_search".into(), query: Some("alien".into()), media_type: None, count: Some(2), titles: None, failed: false });
        db::assert_statement_count(&db::log(conn), 1);
    }

    #[tokio::test]
    async fn run_looks_up_only_current_users_library_when_tmdb_has_hits() {
        init();
        let tmdb = tmdb_returning(vec![full_multi_movie(348)]);
        let conn = db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();
        let user = CurrentUser::from(user_model(4));

        run(&json!({ "query": "alien" }), &user, &conn, &tmdb).await.ok().expect("run");

        let log = db::log(conn);
        db::assert_statement_count(&log, 1);
        assert!(log[0].sql.contains(r#""media"."user_id" = "#), "library lookup not filtered by user: {}", log[0].sql);
        let values = &log[0].values.as_ref().expect("the lookup is bound with values").0;
        assert!(values.contains(&sea_orm::Value::from(4)), "user id 4 not among {values:?}");
    }

    #[tokio::test]
    async fn run_does_not_mark_hit_when_library_has_same_tmdb_id_with_other_type() {
        init();
        let tmdb = tmdb_returning(vec![full_multi_movie(348)]);
        let conn = db::mock().append_query_results([vec![library_row(5, 348, MediaType::Series)]]).into_connection();

        let outcome = run(&json!({ "query": "alien" }), &user(), &conn, &tmdb).await.ok().expect("run");

        assert_eq!(content(&outcome)["results"][0]["mia_id"], Value::Null);
    }

    #[tokio::test]
    async fn run_sends_null_rating_when_hit_has_no_votes() {
        init();
        let unvoted_movie = match full_multi_movie(348) {
            MultiResult::Movie(movie) => MultiResult::Movie(MultiMovieResult { vote_average: Some(0.0), vote_count: Some(0), ..movie }),
            _ => unreachable!(),
        };
        let uncounted_tv = match full_multi_tv(1399) {
            MultiResult::Tv(tv) => MultiResult::Tv(MultiTvResult { vote_average: Some(0.0), vote_count: None, ..tv }),
            _ => unreachable!(),
        };
        let tmdb = tmdb_returning(vec![unvoted_movie, uncounted_tv]);
        let conn = db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();

        let outcome = run(&json!({ "query": "alien" }), &user(), &conn, &tmdb).await.ok().expect("run");

        let results = content(&outcome)["results"].clone();
        assert_eq!(results[0]["tmdb_rating"], Value::Null);
        assert_eq!(results[1]["tmdb_rating"], Value::Null);
    }

    #[tokio::test]
    async fn run_keeps_only_requested_type_when_type_given() {
        init();
        let tmdb = tmdb_returning(vec![full_multi_movie(348), full_multi_tv(1399)]);
        let conn = db::mock().append_query_results([Vec::<media::Model>::new()]).into_connection();

        let outcome = run(&json!({ "query": "alien", "type": "series" }), &user(), &conn, &tmdb).await.ok().expect("run");

        let results = content(&outcome)["results"].clone();
        assert_eq!(results.as_array().expect("array").len(), 1);
        assert_eq!(results[0]["tmdb_id"], 1399);
        assert_eq!(outcome.display.media_type.as_deref(), Some("series"));
        assert_eq!(outcome.display.count, Some(1));
    }

    #[tokio::test]
    async fn run_skips_library_query_when_tmdb_finds_nothing() {
        init();
        let tmdb = tmdb_returning(vec![]);
        let conn = db::mock().into_connection();

        let outcome = run(&json!({ "query": "alien" }), &user(), &conn, &tmdb).await.ok().expect("run");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"results":[]}"#);
        assert_eq!(outcome.display.count, Some(0));
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_without_calling_tmdb_when_type_is_invalid() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run(&json!({ "query": "alien", "type": "tv" }), &user(), &conn, &MockTmdb::new()).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "type must be \"movie\" or \"series\"");
        assert!(outcome.display.failed);
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_without_calling_tmdb_when_query_is_missing() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run(&json!({}), &user(), &conn, &MockTmdb::new()).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "input must have a string 'query'");
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_when_tmdb_fails() {
        init();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().times(1).returning(|_| Err(error("down")));
        let conn = db::mock().into_connection();

        let outcome = run(&json!({ "query": "alien", "type": "movie" }), &user(), &conn, &tmdb).await.ok().expect("run");

        assert!(outcome.is_error);
        assert!(outcome.content.starts_with("TMDB search failed: "), "{}", outcome.content);
        assert!(outcome.content.contains("down"), "{}", outcome.content);
        assert!(outcome.display.failed);
        assert_eq!(outcome.display.query.as_deref(), Some("alien"));
        db::assert_statement_count(&db::log(conn), 0);
    }
}
