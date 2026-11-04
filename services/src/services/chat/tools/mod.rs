//! The tools the model may call during an Exchange: their definitions, the dispatcher that runs a
//! call, and the Tool result block that stores its outcome.

use integrations::openrouter::LlmTool;
use sea_orm::DbConn;
use serde_json::{json, Value};
use views::conversations::ToolCallView;
use views::users::CurrentUser;

use crate::chat::display::DISPLAY_FIELD;
use crate::infrastructure::{Clock, SrvErr, Tmdb};

pub mod date_time;
pub mod mia_details;
pub mod mia_search;
pub mod tmdb_search;

/// What running one tool call produced.
pub struct ToolOutcome {
    /// What the model reads: JSON text for a success, a plain message for an error.
    pub content: String,
    pub is_error: bool,
    /// How the call is shown to the client.
    pub display: ToolCallView
}

/// The tools offered on every LLM call of an Exchange.
pub fn definitions() -> Vec<LlmTool> {
    vec![
        LlmTool {
            name: "mia_search".into(),
            description: mia_search::DESCRIPTION.into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "offset": { "type": "integer", "minimum": 0 },
                    "limit": { "type": "integer", "minimum": 0 }
                },
                "required": ["query"]
            })
        },
        LlmTool {
            name: "mia_details".into(),
            description: mia_details::DESCRIPTION.into(),
            input_schema: json!({
                "type": "object",
                "properties": { "ids": { "type": "array", "items": { "type": "integer" }, "minItems": 1 } },
                "required": ["ids"]
            })
        },
        LlmTool {
            name: "tmdb_search".into(),
            description: tmdb_search::DESCRIPTION.into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "type": { "type": "string", "enum": ["movie", "series"] }
                },
                "required": ["query"]
            })
        },
        LlmTool {
            name: "date_time".into(),
            description: "Gets the current date and time in UTC.".into(),
            input_schema: json!({ "type": "object", "properties": {} })
        },
    ]
}

/// Runs the tool `name` with `input` as `user`. Anything wrong with the call itself becomes an
/// error outcome for the model; `Err` is only for the database failing.
pub async fn run_tool(name: &str, input: &Value, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, clock: &dyn Clock) -> Result<ToolOutcome, SrvErr> {
    match name {
        "date_time" => Ok(date_time::run(clock)),
        "mia_search" => mia_search::run(input, user, db).await,
        "mia_details" => mia_details::run(input, user, db).await,
        "tmdb_search" => tmdb_search::run(input, user, db, tmdb).await,
        _ => Ok(failed(name, input, &format!("unknown tool '{name}'")))
    }
}

/// An error outcome showing `name` and whatever of the call's query and type can be read from
/// `input`.
pub fn failed(name: &str, input: &Value, message: &str) -> ToolOutcome {
    ToolOutcome {
        content: message.into(),
        is_error: true,
        display: ToolCallView {
            name: name.into(),
            query: input["query"].as_str().map(str::to_string),
            media_type: input["type"].as_str().map(str::to_string),
            count: None,
            titles: None,
            failed: true
        }
    }
}

/// The `tool_result` content block that stores `outcome` as the answer to call `tool_use_id`.
pub fn result_block(tool_use_id: &str, outcome: &ToolOutcome) -> Value {
    let mut block = json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": outcome.content,
        DISPLAY_FIELD: outcome.display
    });
    if outcome.is_error {
        block["is_error"] = json!(true);
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::fixtures::user;
    use crate::test_support::tmdb::multi_results;
    use crate::test_support::{db, fixed_clock, init, MockTmdb};

    #[tokio::test]
    async fn run_tool_returns_error_outcome_when_tool_is_unknown() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run_tool("nope", &json!({ "query": "x" }), &user(), &conn, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0))
            .await.ok().expect("run_tool");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "unknown tool 'nope'");
        assert_eq!(outcome.display.name, "nope");
        assert!(outcome.display.failed);
        db::assert_statement_count(&db::log(conn), 0);
    }

    #[tokio::test]
    async fn run_tool_returns_current_utc_time_when_date_time() {
        init();
        let conn = db::mock().into_connection();

        let outcome = run_tool("date_time", &json!({}), &user(), &conn, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0))
            .await.ok().expect("run_tool");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"utc":"2024-01-01T12:00:00Z","timezone":"UTC"}"#);
        assert_eq!(outcome.display, ToolCallView { name: "date_time".into(), query: None, media_type: None, count: None, titles: None, failed: false });
    }

    #[tokio::test]
    async fn run_tool_runs_mia_details_when_called() {
        init();
        let conn = db::mock().append_query_results([Vec::<entities::media::Model>::new()]).into_connection();

        let outcome = run_tool("mia_details", &json!({ "ids": [7] }), &user(), &conn, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0))
            .await.ok().expect("run_tool");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"media":[],"not_found":[7]}"#);
        assert_eq!(outcome.display.name, "mia_details");
    }

    #[tokio::test]
    async fn run_tool_runs_tmdb_search_when_called() {
        init();
        let conn = db::mock().into_connection();
        let mut tmdb = MockTmdb::new();
        tmdb.expect_search_multi().times(1).returning(|_| Ok(multi_results(vec![])));

        let outcome = run_tool("tmdb_search", &json!({ "query": "alien" }), &user(), &conn, &tmdb, &fixed_clock(2024, 1, 1, 12, 0, 0))
            .await.ok().expect("run_tool");

        assert!(!outcome.is_error);
        assert_eq!(outcome.content, r#"{"results":[]}"#);
        assert_eq!(outcome.display.name, "tmdb_search");
    }

    #[test]
    fn failed_shows_query_and_type_when_input_has_them() {
        let outcome = failed("tmdb_search", &json!({ "query": "alien", "type": "movie" }), "boom");

        assert!(outcome.is_error);
        assert_eq!(outcome.content, "boom");
        assert_eq!(outcome.display.query.as_deref(), Some("alien"));
        assert_eq!(outcome.display.media_type.as_deref(), Some("movie"));
    }

    #[test]
    fn failed_omits_query_and_type_when_input_values_are_not_strings() {
        let outcome = failed("mia_search", &json!({ "query": 5, "type": ["movie"] }), "boom");

        assert_eq!(outcome.display.query, None);
        assert_eq!(outcome.display.media_type, None);
    }

    #[test]
    fn result_block_includes_display_and_error_flag_when_outcome_failed() {
        let outcome = failed("mia_search", &json!({ "query": "x" }), "bad query");

        let block = result_block("toolu_1", &outcome);

        assert_eq!(block, json!({
            "type": "tool_result",
            "tool_use_id": "toolu_1",
            "content": "bad query",
            "is_error": true,
            "mia_display": { "name": "mia_search", "query": "x", "failed": true }
        }));
    }

    #[test]
    fn result_block_omits_is_error_when_outcome_succeeded() {
        let outcome = date_time::run(&fixed_clock(2024, 1, 1, 12, 0, 0));

        let block = result_block("toolu_2", &outcome);

        assert!(block.get("is_error").is_none());
        assert_eq!(block["content"], outcome.content);
        assert_eq!(block["mia_display"], json!({ "name": "date_time", "failed": false }));
    }

    #[test]
    fn definitions_offer_four_tools_with_object_schemas() {
        let tools = definitions();

        let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, ["mia_search", "mia_details", "tmdb_search", "date_time"]);
        for tool in &tools {
            assert_eq!(tool.input_schema["type"], "object", "{}", tool.name);
        }
        let tmdb_search = &tools[2].input_schema;
        assert_eq!(tmdb_search["properties"]["type"]["enum"], json!(["movie", "series"]));
        assert_eq!(tools[3].input_schema, json!({ "type": "object", "properties": {} }));
    }

    #[test]
    fn definitions_let_mia_search_take_optional_offset_and_limit() {
        let schema = &definitions()[0].input_schema;

        assert_eq!(schema["required"], json!(["query"]));
        for field in ["offset", "limit"] {
            assert_eq!(schema["properties"][field]["type"], "integer", "{field}");
            assert_eq!(schema["properties"][field]["minimum"], 0, "{field}");
        }
    }
}
