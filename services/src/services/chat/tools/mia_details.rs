//! The `mia_details` tool: the details of media in the user's own library.

use entities::prelude::Media;
use entities::sea_orm_active_enums::MediaType;
use entities::media;
use infrastructure::config;
use sea_orm::{ColumnTrait, DbConn, EntityTrait, QueryFilter};
use serde_json::{json, Value};
use std::collections::HashSet;
use views::conversations::ToolCallView;
use views::users::CurrentUser;

use crate::chat::tools::{failed, ToolOutcome};
use crate::infrastructure::SrvErr;
use crate::media_details::load_parts;
use crate::{movies, series};

const NAME: &str = "mia_details";

/// What the model is told about the tool.
pub const DESCRIPTION: &str = "Gets the details of media in the user's library: title, alternative titles, overview, \
release or first air date, status, runtime (movies), season and episode counts (series), genres, tags, sources, the \
user's logs and stars, watchlist state, original language and TMDB id. The ids are the `id` of a mia_search hit or \
the `mia_id` of a tmdb_search hit. Takes a limited number of ids per call. Details are at series level only: there \
are no seasons or episodes. Ids that are not in the library come back in `not_found`.";

/// Loads the details of the media `input["ids"]` owned by `user`, with a fixed number of queries
/// (at most 12) whatever the number of ids.
pub async fn run(input: &Value, user: &CurrentUser, db: &DbConn) -> Result<ToolOutcome, SrvErr> {
    let Some(ids) = input["ids"].as_array().and_then(|ids| ids.iter().map(|id| id.as_i64().and_then(|id| i32::try_from(id).ok())).collect::<Option<Vec<i32>>>()) else {
        return Ok(failed(NAME, input, "input must have an integer array 'ids'"));
    };
    if ids.is_empty() {
        return Ok(failed(NAME, input, "ids must not be empty"));
    }
    let mut seen = HashSet::new();
    let ids: Vec<i32> = ids.into_iter().filter(|id| seen.insert(*id)).collect();
    let cap = config().assistant.mia_details_limit as usize;
    if ids.len() > cap {
        return Ok(failed(NAME, input, &format!("at most {cap} ids per call")));
    }

    let mut found: Vec<media::Model> = Media::find().filter(media::Column::Id.is_in(ids.clone()))
        .filter(media::Column::UserId.eq(user.id)).all(db).await?;
    found.sort_by_key(|media| ids.iter().position(|id| *id == media.id));

    let movie_ids: Vec<i32> = found.iter().filter(|media| media.r#type == MediaType::Movie).map(|media| media.id).collect();
    let series_ids: Vec<i32> = found.iter().filter(|media| media.r#type == MediaType::Series).map(|media| media.id).collect();
    let mut parts = load_parts(&found, db).await?;
    let mut movie_rows = if movie_ids.is_empty() { Default::default() } else { movies::load_movies(&movie_ids, db).await? };
    let (mut series_rows, counts) = if series_ids.is_empty() {
        (Default::default(), Default::default())
    } else {
        (series::load_series(&series_ids, db).await?, series::load_counts(&series_ids, db).await?)
    };

    let mut titles = Vec::new();
    let mut details = Vec::new();
    for media in found {
        let id = media.id;
        let media_parts = parts.remove(&id).ok_or_else(|| SrvErr::Internal(format!("media {id} has no loaded parts")))?;
        let (media_type, view) = match media.r#type {
            MediaType::Movie => {
                let movie = movie_rows.remove(&id).ok_or_else(|| SrvErr::Internal(format!("media {id} has no movie row")))?;
                ("movie", serde_json::to_value(movies::details_view(media, movie, media_parts, vec![], None, None)))
            }
            MediaType::Series => {
                let series = series_rows.remove(&id).ok_or_else(|| SrvErr::Internal(format!("media {id} has no series row")))?;
                let count = *counts.get(&id).ok_or_else(|| SrvErr::Internal(format!("series {id} has no season and episode counts")))?;
                ("series", serde_json::to_value(series::details_view(media, series, media_parts, count, vec![], None, None)))
            }
        };
        let mut view = view.map_err(|err| SrvErr::Internal(format!("details of media {id} do not serialize: {err}")))?;
        titles.push(view["title"].as_str().unwrap_or_default().to_string());
        if let Some(object) = view.as_object_mut() {
            for key in ["poster", "backdrop", "locks"] {
                object.remove(key);
            }
            object.insert("type".into(), json!(media_type));
        }
        details.push(view);
    }

    let found_ids: HashSet<i64> = details.iter().filter_map(|view| view["id"].as_i64()).collect();
    let not_found: Vec<i32> = ids.into_iter().filter(|id| !found_ids.contains(&i64::from(*id))).collect();
    let display = ToolCallView { name: NAME.into(), query: None, media_type: None, count: None, titles: Some(titles), failed: false };
    Ok(ToolOutcome { content: json!({ "media": details, "not_found": not_found }).to_string(), is_error: false, display })
}

#[cfg(test)]
mod tests {
    use super::*;
    use entities::media::Model as MediaModel;
    use crate::test_support::db::{assert_statement_count, assert_wrote, log, mock};
    use crate::test_support::fixtures;
    use crate::test_support::init;
    use sea_orm::MockDatabase;

    fn english(media: MediaModel) -> MediaModel {
        MediaModel { original_language: Some("en".into()), ..media }
    }

    /// Queues the 7 queries of `load_parts` (the languages query included) with one title per
    /// media.
    fn with_parts(db: MockDatabase, media: &[(i32, &str)]) -> MockDatabase {
        db.append_query_results([media.iter().map(|(id, title)| fixtures::title_model(*id, *id, title, true)).collect::<Vec<_>>()])
            .append_query_results([[fixtures::language_model("en", "English")]])
            .append_query_results([Vec::<(entities::media_genres::Model, Option<entities::genres::Model>)>::new()])
            .append_query_results([Vec::<(entities::media_tags::Model, Option<entities::tags::Model>)>::new()])
            .append_query_results([Vec::<entities::sources::Model>::new()])
            .append_query_results([Vec::<entities::logs::Model>::new()])
            .append_query_results([Vec::<entities::watchlist::Model>::new()])
    }

    fn media_ids(content: &Value) -> Vec<i64> {
        content["media"].as_array().expect("media array").iter().map(|m| m["id"].as_i64().expect("id")).collect()
    }

    #[tokio::test]
    async fn run_returns_details_without_images_and_locks_in_fixed_query_count_when_movies_and_series_found() {
        init();
        // The lookup returns rows in table order, not request order.
        let lookup = [english(fixtures::series_media_model(9, 1)), english(fixtures::movie_media_model(5, 1)), english(fixtures::movie_media_model(6, 1))];
        let conn = with_parts(mock().append_query_results([lookup]), &[(5, "Alien"), (6, "Aliens"), (9, "Andor")])
            .append_query_results([[fixtures::movie_model(5), fixtures::movie_model(6)]])
            .append_query_results([[fixtures::series_model(9)]])
            .append_query_results([[fixtures::series_count_row(9, 2)]])
            .append_query_results([[fixtures::series_count_row(9, 20)]])
            .into_connection();

        let outcome = run(&json!({ "ids": [5, 6, 9] }), &fixtures::user(), &conn).await.ok().expect("run");

        assert!(!outcome.is_error);
        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(media_ids(&content), [5, 6, 9]);
        let types: Vec<&str> = content["media"].as_array().unwrap().iter().map(|m| m["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["movie", "movie", "series"]);
        for media in content["media"].as_array().unwrap() {
            for key in ["poster", "backdrop", "locks"] {
                assert!(media.get(key).is_none(), "{key} present in {media}");
            }
        }
        assert_eq!(content["media"][0]["title"], "Alien");
        assert_eq!(content["media"][2]["number_of_seasons"], 2);
        assert_eq!(content["media"][2]["number_of_episodes"], 20);
        assert_eq!(content["not_found"], json!([]));
        assert_eq!(outcome.display, ToolCallView {
            name: "mia_details".into(), query: None, media_type: None, count: None,
            titles: Some(vec!["Alien".into(), "Aliens".into(), "Andor".into()]), failed: false
        });
        assert_statement_count(&log(conn), 12);
    }

    #[tokio::test]
    async fn run_skips_series_queries_when_only_movies_found() {
        init();
        let conn = with_parts(mock().append_query_results([[english(fixtures::movie_media_model(5, 1))]]), &[(5, "Alien")])
            .append_query_results([[fixtures::movie_model(5)]])
            .into_connection();

        let outcome = run(&json!({ "ids": [5] }), &fixtures::user(), &conn).await.ok().expect("run");

        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(media_ids(&content), [5]);
        assert_statement_count(&log(conn), 9);
    }

    #[tokio::test]
    async fn run_lists_missing_ids_as_not_found_when_lookup_misses() {
        init();
        let conn = mock().append_query_results([Vec::<MediaModel>::new()]).into_connection();

        let outcome = run(&json!({ "ids": [7] }), &fixtures::user(), &conn).await.ok().expect("run");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"media":[],"not_found":[7]}"#);
        assert_eq!(outcome.display.titles, Some(vec![]));
        let log = log(conn);
        assert_statement_count(&log, 1);
        assert_wrote(&log, "SELECT", &[sea_orm::Value::from(7), sea_orm::Value::from(1)]);
    }

    #[tokio::test]
    async fn run_looks_up_only_current_users_media_when_ids_given() {
        init();
        let conn = mock().append_query_results([Vec::<MediaModel>::new()]).into_connection();
        let user = CurrentUser::from(fixtures::user_model(4));

        run(&json!({ "ids": [7] }), &user, &conn).await.ok().expect("run");

        let log = log(conn);
        assert_statement_count(&log, 1);
        assert!(log[0].sql.contains(r#""media"."user_id" = "#), "lookup not filtered by user: {}", log[0].sql);
        let values = &log[0].values.as_ref().expect("the lookup is bound with values").0;
        assert!(values.contains(&sea_orm::Value::from(4)), "user id 4 not among {values:?}");
    }

    #[tokio::test]
    async fn run_skips_movie_queries_when_only_series_found() {
        init();
        let conn = with_parts(mock().append_query_results([[english(fixtures::series_media_model(9, 1))]]), &[(9, "Andor")])
            .append_query_results([[fixtures::series_model(9)]])
            .append_query_results([fixtures::no_series_count_rows()])
            .append_query_results([fixtures::no_series_count_rows()])
            .into_connection();

        let outcome = run(&json!({ "ids": [9] }), &fixtures::user(), &conn).await.ok().expect("run");

        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(media_ids(&content), [9]);
        assert_eq!(content["media"][0]["type"], "series");
        assert_eq!(content["media"][0]["number_of_seasons"], 0);
        assert_statement_count(&log(conn), 11);
    }

    #[tokio::test]
    async fn run_lists_only_missing_ids_as_not_found_when_some_ids_found() {
        init();
        let lookup = [english(fixtures::movie_media_model(5, 1)), english(fixtures::movie_media_model(6, 1))];
        let conn = with_parts(mock().append_query_results([lookup]), &[(5, "Alien"), (6, "Aliens")])
            .append_query_results([[fixtures::movie_model(5), fixtures::movie_model(6)]])
            .into_connection();

        let outcome = run(&json!({ "ids": [6, 8, 5] }), &fixtures::user(), &conn).await.ok().expect("run");

        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(media_ids(&content), [6, 5]);
        assert_eq!(content["not_found"], json!([8]));
        assert_eq!(outcome.display.titles, Some(vec!["Aliens".into(), "Alien".into()]));
    }

    #[tokio::test]
    async fn run_returns_internal_error_when_movie_row_missing() {
        init();
        let conn = with_parts(mock().append_query_results([[english(fixtures::movie_media_model(5, 1))]]), &[(5, "Alien")])
            .append_query_results([Vec::<entities::movies::Model>::new()])
            .into_connection();

        let result = run(&json!({ "ids": [5] }), &fixtures::user(), &conn).await;

        assert!(matches!(result, Err(SrvErr::Internal(_))));
    }

    #[tokio::test]
    async fn run_returns_internal_error_when_series_row_missing() {
        init();
        let conn = with_parts(mock().append_query_results([[english(fixtures::series_media_model(9, 1))]]), &[(9, "Andor")])
            .append_query_results([Vec::<entities::series::Model>::new()])
            .append_query_results([fixtures::no_series_count_rows()])
            .append_query_results([fixtures::no_series_count_rows()])
            .into_connection();

        let result = run(&json!({ "ids": [9] }), &fixtures::user(), &conn).await;

        assert!(matches!(result, Err(SrvErr::Internal(_))));
    }

    #[tokio::test]
    async fn run_returns_error_when_ids_exceed_cap() {
        init();
        let conn = mock().into_connection();

        let outcome = run(&json!({ "ids": [1, 2, 3, 4] }), &fixtures::user(), &conn).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "at most 3 ids per call");
        assert!(outcome.display.failed);
        assert_statement_count(&log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_when_ids_empty() {
        init();
        let conn = mock().into_connection();

        let outcome = run(&json!({ "ids": [] }), &fixtures::user(), &conn).await.ok().expect("run");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "ids must not be empty");
        assert_statement_count(&log(conn), 0);
    }

    #[tokio::test]
    async fn run_returns_error_when_ids_is_not_an_integer_array() {
        init();
        let conn = mock().into_connection();

        for input in [json!({}), json!({ "ids": "5" }), json!({ "ids": [1, "2"] }), json!({ "ids": [1.5] })] {
            let outcome = run(&input, &fixtures::user(), &conn).await.ok().expect("run");

            assert!(outcome.is_error, "{input}");
            assert_eq!(outcome.content, "input must have an integer array 'ids'");
        }
        assert_statement_count(&log(conn), 0);
    }

    #[tokio::test]
    async fn run_looks_up_each_id_once_when_ids_repeat() {
        init();
        let conn = mock().append_query_results([Vec::<MediaModel>::new()]).into_connection();

        let outcome = run(&json!({ "ids": [7, 7] }), &fixtures::user(), &conn).await.ok().expect("run");

        let content: Value = serde_json::from_str(&outcome.content).expect("content is JSON");
        assert_eq!(content["not_found"], json!([7]));
        assert_statement_count(&log(conn), 1);
    }

    #[tokio::test]
    async fn run_counts_distinct_ids_against_cap_when_ids_repeat() {
        init();
        let conn = mock().append_query_results([Vec::<MediaModel>::new()]).into_connection();

        let outcome = run(&json!({ "ids": [1, 1, 2, 2, 3, 3] }), &fixtures::user(), &conn).await.ok().expect("run");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"media":[],"not_found":[1,2,3]}"#);
    }
}
