use sea_orm::{DatabaseBackend, DatabaseConnection, MockDatabase, MockExecResult, Statement, Value};
use entities::images;
use entities::sea_orm_active_enums::{ExchangeStatus, ImageType, MessageRole};
use serde_json::json;
use super::fixtures;

/// A mock Postgres database. Queue results with `append_query_results` / `append_exec_results`,
/// then call `into_connection()`.
///
/// Inserts on Postgres use `RETURNING`, so they consume *query* results, not exec results.
pub fn mock() -> MockDatabase {
    MockDatabase::new(DatabaseBackend::Postgres)
}

/// Consumes the connection and returns every statement it executed, in order.
pub fn log(db: DatabaseConnection) -> Vec<Statement> {
    db.into_transaction_log()
        .into_iter()
        .flat_map(|transaction| transaction.statements().to_vec())
        .collect()
}

pub fn assert_statement_count(log: &[Statement], n: usize) {
    assert_eq!(log.len(), n, "expected {n} statements, got:\n{}", describe(log));
}

/// Asserts that some statement's SQL starts with `sql_prefix` (e.g. `INSERT INTO "tags"`) and
/// that its bound values contain every one of `expected_values`.
pub fn assert_wrote(log: &[Statement], sql_prefix: &str, expected_values: &[Value]) {
    let found = log.iter().any(|statement| {
        statement.sql.starts_with(sql_prefix)
            && expected_values.iter().all(|expected| {
                statement.values.as_ref().is_some_and(|values| values.0.contains(expected))
            })
    });
    assert!(
        found,
        "no statement starting with {sql_prefix:?} contains values {expected_values:?}; logged statements:\n{}",
        describe(log)
    );
}

/// The first bound string value that starts with `value_prefix` in a statement whose SQL starts
/// with `sql_prefix`; for reading back generated values such as password hashes.
pub fn string_value_written(log: &[Statement], sql_prefix: &str, value_prefix: &str) -> Option<String> {
    log.iter()
        .filter(|statement| statement.sql.starts_with(sql_prefix))
        .filter_map(|statement| statement.values.as_ref())
        .flat_map(|values| values.0.iter())
        .find_map(|value| match value {
            Value::String(Some(s)) if s.starts_with(value_prefix) => Some(s.to_string()),
            _ => None,
        })
}

/// Asserts that no statement's SQL starts with `sql_prefix`.
pub fn assert_no_statement_starting_with(log: &[Statement], sql_prefix: &str) {
    assert!(
        !log.iter().any(|statement| statement.sql.starts_with(sql_prefix)),
        "unexpected statement starting with {sql_prefix:?}; logged statements:\n{}",
        describe(log)
    );
}

/// The result of a `DELETE` that removed one row; queue it with `append_exec_results`.
pub fn deleted() -> MockExecResult {
    MockExecResult { last_insert_id: 0, rows_affected: 1 }
}

/// The result of an `UPDATE` that changed `rows` rows; queue it with `append_exec_results`.
pub fn updated(rows: u64) -> MockExecResult {
    MockExecResult { last_insert_id: 0, rows_affected: rows }
}

/// Queues what `images::save_tmdb_image` reads and writes for a new 300x450 image: the lookup by
/// source path (nothing found), the directory clash check, the image row and its original size row.
/// The image row has id `image_id` and type `image_type`; the size row has the same id.
pub fn with_saved_tmdb_image(db: MockDatabase, image_id: i32, image_type: ImageType) -> MockDatabase {
    db.append_query_results([Vec::<images::Model>::new()])
        .append_query_results([[fixtures::count_row(0)]])
        .append_query_results([[images::Model { r#type: image_type, ..fixtures::image_model(image_id) }]])
        .append_query_results([[fixtures::image_size_model(image_id, image_id, 300, 450)]])
}

/// Queues what `images::delete_unused_image` reads and writes when it deletes image `image_id`: the
/// four usage counts (all zero), the image row, and the deletes of its size rows and of the image.
/// The caller must have put a file in the image's directory (`image-{image_id}`) in the in-memory store.
pub fn with_deleted_unused_image(db: MockDatabase, image_id: i32) -> MockDatabase {
    db.append_query_results([[fixtures::count_row(0)]])
        .append_query_results([[fixtures::count_row(0)]])
        .append_query_results([[fixtures::count_row(0)]])
        .append_query_results([[fixtures::count_row(0)]])
        .append_query_results([[fixtures::image_model(image_id)]])
        .append_exec_results([deleted(), deleted()])
}

/// Queues what `chat::exchange::run_exchange` reads and writes before it calls the model: in-progress
/// Exchange `exchange_id` of Conversation 3 (user 1), that Conversation, its only Exchange, the
/// Exchange's user Message (id 1, "Hi") and the inserted LLM call (id 11).
pub fn with_started_exchange_run(db: MockDatabase, exchange_id: i32) -> MockDatabase {
    let exchange = fixtures::exchange_model(exchange_id, 3, ExchangeStatus::InProgress);
    db.append_query_results([[exchange.clone()]])
        .append_query_results([[fixtures::conversation_model(3, 1)]])
        .append_query_results([[exchange]])
        .append_query_results([[fixtures::message_model(1, exchange_id, MessageRole::User, json!([{"type": "text", "text": "Hi"}]))]])
        .append_query_results([[fixtures::llm_call_model(11, exchange_id)]])
}

fn describe(log: &[Statement]) -> String {
    if log.is_empty() {
        return "  (none)".to_string();
    }
    log.iter()
        .enumerate()
        .map(|(i, statement)| {
            let values = statement.values.as_ref().map_or_else(Vec::new, |v| v.0.clone());
            format!("  {i}: {} {values:?}", statement.sql)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
