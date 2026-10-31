//! The `mia_search` tool: searches the user's own library.

use chrono::{Datelike, NaiveDate};
use entities::sea_orm_active_enums::MediaType;
use entities::{movies, series};
use infrastructure::config;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter};
use serde_json::{json, Value};
use std::collections::HashMap;
use views::conversations::ToolCallView;
use views::media::{PageReq, SearchQuery, SortTarget};
use views::users::CurrentUser;

use crate::chat::tools::{failed, ToolOutcome};
use crate::infrastructure::SrvErr;
use crate::media::search_library;

const NAME: &str = "mia_search";

/// What the model is told about the tool and the query language.
pub const DESCRIPTION: &str = include_str!("mia_search.md");

/// Runs `input["query"]` against `user`'s library and returns at most the configured number of
/// hits, each with its release year.
pub async fn run(input: &Value, user: &CurrentUser, db: &DbConn) -> Result<ToolOutcome, SrvErr> {
    let Some(query) = input["query"].as_str() else {
        return Ok(failed(NAME, input, "input must have a string 'query'"));
    };
    let search = SearchQuery { query: query.to_string(), genres: None, only_watched: false, min_stars: None, sort_by: SortTarget::Title };
    let page = PageReq { offset: None, limit: Some(config().assistant.mia_search_limit as u64) };
    let library = match search_library(search, page, None, user, db).await? {
        Ok(library) => library,
        Err(messages) => return Ok(failed(NAME, input, &messages.join("\n"))),
    };

    let ids_of = |wanted: MediaType| library.rows.iter().filter(|(media, _, _)| media.r#type == wanted).map(|(media, _, _)| media.id).collect::<Vec<_>>();
    let release_dates = release_dates(ids_of(MediaType::Movie), ids_of(MediaType::Series), db).await?;
    let results: Vec<Value> = library.rows.iter().map(|(media, _, title)| json!({
        "id": media.id,
        "type": match media.r#type { MediaType::Movie => "movie", MediaType::Series => "series" },
        "title": title.as_ref().map_or_else(|| config().media.unset_title.clone(), |title| title.title.clone()),
        "release_year": release_dates.get(&media.id).map(|date| date.year()),
        "overview": media.overview,
        "tmdb_id": media.tmdb_id,
        "stars": media.stars.and_then(|stars| stars.to_string().parse::<f64>().ok()),
    })).collect();

    let display = ToolCallView { name: NAME.into(), query: Some(query.to_string()), media_type: None, count: Some(results.len() as u32), titles: None, failed: false };
    Ok(ToolOutcome { content: json!({ "results": results }).to_string(), is_error: false, display })
}

/// The release date of each of the given movie and series ids that has one, from `movies` and
/// `series`: one query per non-empty list.
async fn release_dates(movie_ids: Vec<i32>, series_ids: Vec<i32>, db: &DbConn) -> Result<HashMap<i32, NaiveDate>, SrvErr> {
    let mut dates = HashMap::new();
    if !movie_ids.is_empty() {
        let rows = movies::Entity::find().filter(movies::Column::Id.is_in(movie_ids)).all(db).await?;
        dates.extend(rows.into_iter().filter_map(|row| Some((row.id, row.release_date?))));
    }
    if !series_ids.is_empty() {
        let rows = series::Entity::find().filter(series::Column::Id.is_in(series_ids)).all(db).await?;
        dates.extend(rows.into_iter().filter_map(|row| Some((row.id, row.first_air_date?))));
    }
    Ok(dates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use entities::sea_orm_active_enums::MediaType;
    use sea_orm::Value as DbValue;
    use serde_json::json;
    use views::conversations::ToolCallView;
    use views::media::{PageReq, SearchQuery, SortTarget};

    use crate::test_support::fixtures::{media_index_row, media_model, movie_model, series_model, title_model, user};
    use crate::test_support::{date, db, init};

    fn movie_hit() -> std::collections::BTreeMap<String, DbValue> {
        let mut media = media_model(1, 1, MediaType::Movie);
        media.overview = Some("Space".into());
        media.tmdb_id = Some(348);
        media.stars = Some(4.5);
        media_index_row(media, None, Some(title_model(1, 1, "Alien", true)))
    }

    #[test]
    fn description_names_every_filter_field_and_sort_target() {
        for name in transpiler::FILTER_FIELDS.iter().chain(transpiler::SORT_TARGETS.iter()) {
            assert!(DESCRIPTION.contains(name), "description does not mention {name}");
        }
    }

    #[test]
    fn description_examples_transpile_when_run_through_the_transpiler() {
        init();
        let examples = DESCRIPTION.split("Examples").nth(1).expect("examples section").split("\n\n").next().expect("examples block");
        let queries: Vec<String> = examples.lines().filter(|line| line.trim_start().starts_with('"'))
            .map(|line| serde_json::Deserializer::from_str(line.trim_start()).into_iter::<String>().next().expect("example starts with a string").expect("example is a JSON string"))
            .collect();
        assert!(queries.len() >= 6, "found only {queries:?}");

        for query in queries {
            let search = SearchQuery { query: query.clone(), genres: None, only_watched: false, min_stars: None, sort_by: SortTarget::Title };
            if let Err(errors) = transpiler::transpile(search, PageReq { offset: None, limit: Some(30) }, &user(), None) {
                let messages: Vec<String> = errors.into_iter().map(|error| format!("{}: {}", error.source, error.message)).collect();
                panic!("example {query:?} does not transpile: {messages:?}");
            }
        }
    }

    #[tokio::test]
    async fn run_returns_hits_with_release_year_when_query_matches() {
        init();
        let mut movie = movie_model(1);
        movie.release_date = Some(date(1979, 5, 25));
        let conn = db::mock()
            .append_query_results([vec![movie_hit()]])
            .append_query_results([vec![movie]])
            .into_connection();

        let outcome = run(&json!({ "query": "alien" }), &user(), &conn).await.ok().expect("run");

        assert!(!outcome.is_error);
        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(content, json!({ "results": [{
            "id": 1, "type": "movie", "title": "Alien", "release_year": 1979,
            "overview": "Space", "tmdb_id": 348, "stars": 4.5
        }] }));
        assert_eq!(outcome.display, ToolCallView { name: "mia_search".into(), query: Some("alien".into()), media_type: None, count: Some(1), titles: None, failed: false });
        db::assert_statement_count(&db::log(conn), 2);
    }

    #[tokio::test]
    async fn run_loads_release_dates_with_one_query_per_type_when_hits_are_mixed() {
        init();
        let series_hit = media_index_row(media_model(2, 1, MediaType::Series), None, None);
        let other_series_hit = media_index_row(media_model(3, 1, MediaType::Series), None, Some(title_model(3, 3, "Show", true)));
        let mut series = series_model(2);
        series.first_air_date = Some(date(2008, 1, 20));
        let mut no_date = series_model(3);
        no_date.first_air_date = None;
        let conn = db::mock()
            .append_query_results([vec![movie_hit(), series_hit, other_series_hit]])
            .append_query_results([vec![movie_model(1)]])
            .append_query_results([vec![series, no_date]])
            .into_connection();

        let outcome = run(&json!({ "query": "" }), &user(), &conn).await.ok().expect("run");

        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        let results = content["results"].as_array().expect("results");
        assert_eq!(results.len(), 3);
        assert_eq!(results[1]["type"], "series");
        assert_eq!(results[1]["title"], "Unknown");
        assert_eq!(results[1]["release_year"], 2008);
        assert_eq!(results[1]["overview"], Value::Null);
        assert_eq!(results[2]["release_year"], Value::Null);
        db::assert_statement_count(&db::log(conn), 3);
    }

    #[tokio::test]
    async fn run_skips_release_date_queries_when_there_are_no_hits() {
        init();
        let conn = db::mock().append_query_results([Vec::<std::collections::BTreeMap<String, DbValue>>::new()]).into_connection();

        let outcome = run(&json!({ "query": "nothing" }), &user(), &conn).await.ok().expect("run");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"results":[]}"#);
        assert_eq!(outcome.display.count, Some(0));
        db::assert_statement_count(&db::log(conn), 1);
    }

    #[tokio::test]
    async fn run_returns_error_with_transpile_messages_when_query_is_invalid() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run(&json!({ "query": " : foo = 1" }), &user(), &conn).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "Construction: Unknown target 'foo'");
        assert_eq!(outcome.display, ToolCallView { name: "mia_search".into(), query: Some(" : foo = 1".into()), media_type: None, count: None, titles: None, failed: true });
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_when_query_is_missing() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run(&json!({}), &user(), &conn).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "input must have a string 'query'");
        assert!(outcome.display.failed);
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_when_query_is_not_a_string() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run(&json!({ "query": 5 }), &user(), &conn).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "input must have a string 'query'");
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_first_page_of_library_when_query_is_empty() {
        init();
        let mut movie = movie_model(1);
        movie.release_date = Some(date(1979, 5, 25));
        let conn = db::mock()
            .append_query_results([vec![movie_hit()]])
            .append_query_results([vec![movie]])
            .into_connection();

        let outcome = run(&json!({ "query": "" }), &user(), &conn).await.ok().expect("run");

        assert!(!outcome.is_error);
        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(content["results"][0]["title"], "Alien");
        assert_eq!(outcome.display.query.as_deref(), Some(""));
        assert_eq!(outcome.display.count, Some(1));
    }

    #[tokio::test]
    async fn run_limits_results_to_configured_cap() {
        init();
        let conn = db::mock().append_query_results([Vec::<std::collections::BTreeMap<String, DbValue>>::new()]).into_connection();

        run(&json!({ "query": "alien" }), &user(), &conn).await.ok().expect("run");

        let log = db::log(conn);
        db::assert_statement_count(&log, 1);
        let values = &log[0].values.as_ref().expect("the search is bound with values").0;
        assert!(values.contains(&DbValue::BigUnsigned(Some(30))), "no LIMIT of 30 among {values:?}");
    }
}
