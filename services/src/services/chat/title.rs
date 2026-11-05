//! Titling a Conversation: a second, smaller model names it after its first user Message.

use chrono::{DateTime, FixedOffset};
use entities::sea_orm_active_enums::{LlmCallPurpose, MessageRole};
use entities::{conversations, exchanges, llm_calls, messages};
use futures::StreamExt;
use integrations::openrouter::{Llm, LlmError, LlmMessage, LlmRequest, LlmRole, StopReason};
use log::warn;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, QueryFilter, QueryOrder};
use serde_json::{json, Value};
use tokio::time::timeout;

use crate::chat::accumulator::Accumulator;
use crate::chat::display::user_text;
use crate::chat::exchange::{idle_error, tokens, unfinished_error, IDLE_TIMEOUT};
use crate::chat::failure::{debug_message, llm_call_error};
use crate::chat::registry::ExchangeRegistry;
use crate::infrastructure::{Clock, SrvErr};

pub const TITLE_PROMPT: &str = include_str!("title_prompt.md");

/// The most `char`s a title keeps.
pub const MAX_TITLE_CHARS: usize = 80;

/// Asks `title_llm` for a title for Conversation `conversation_id` from its first user Message,
/// stores it and publishes it to the Conversation's title channel, which is closed when this
/// returns or panics. The call is recorded in `llm_calls` like an Exchange's.
///
/// Every failure (provider error, idle stream, refusal, no usable title, database error) is logged
/// and publishes nothing. Nothing is published either when the Conversation is deleted meanwhile.
pub async fn generate_title(conversation_id: i32, db: &DbConn, title_llm: &dyn Llm, registry: &ExchangeRegistry, clock: &dyn Clock) {
    let _close = CloseTitleOnDrop { registry, conversation_id };
    match title(conversation_id, db, title_llm, clock).await {
        Ok(Some(title)) => registry.publish_title(conversation_id, title),
        Ok(None) => {}
        Err(err) => warn!("Titling Conversation {conversation_id} failed on our side: {err}"),
    }
}

/// Closes the Conversation's title channel when dropped, so the creating response ends even when
/// the job returns early or panics. After a publish the channel is already gone and this does nothing.
struct CloseTitleOnDrop<'a> {
    registry: &'a ExchangeRegistry,
    conversation_id: i32,
}

impl Drop for CloseTitleOnDrop<'_> {
    fn drop(&mut self) {
        self.registry.close_title(self.conversation_id);
    }
}

/// Makes and records the title call, then writes the title. Returns the title to publish: `None`
/// when the Conversation or its first user Message is missing, the call failed, or the
/// Conversation was deleted before the title was written.
async fn title(conversation_id: i32, db: &DbConn, title_llm: &dyn Llm, clock: &dyn Clock) -> Result<Option<String>, SrvErr> {
    let Some(conversation) = conversations::Entity::find_by_id(conversation_id).one(db).await? else {
        return Ok(None);
    };
    let Some(message) = first_user_message(conversation_id, db).await? else {
        warn!("Conversation {conversation_id} has no user message to title it by");
        return Ok(None);
    };

    let call = llm_calls::ActiveModel {
        user_id: Set(Some(conversation.user_id)),
        conversation_id: Set(Some(conversation_id)),
        exchange_id: Set(None),
        purpose: Set(LlmCallPurpose::Title),
        model: Set(title_llm.model()),
        created_at: Set(clock.now().fixed_offset()),
        ..Default::default()
    }
    .insert(db)
    .await?;
    let mut accumulator = Accumulator::new(0);
    let answer = ask(request(&user_text(&message)), title_llm, &mut accumulator).await;
    let call_error = match &answer {
        Err(NoTitle::Failed(error)) => Some(llm_call_error(error)),
        Ok(_) | Err(NoTitle::Unusable(_)) => None,
    };
    close_call(call.id, &accumulator, call_error, clock.now().fixed_offset(), db).await?;
    let title = match answer {
        Ok(title) => title,
        Err(NoTitle::Failed(error)) => {
            warn!("Titling Conversation {conversation_id} failed: {}", debug_message(&error));
            return Ok(None);
        }
        Err(NoTitle::Unusable(reason)) => {
            warn!("Titling Conversation {conversation_id} failed: {reason}");
            return Ok(None);
        }
    };

    // `updated_at` stays as it is, so a title does not reorder the Conversation list.
    let updated = conversations::Entity::update_many()
        .set(conversations::ActiveModel { title: Set(Some(title.clone())), ..Default::default() })
        .filter(conversations::Column::Id.eq(conversation_id))
        .exec(db)
        .await?;
    Ok((updated.rows_affected > 0).then_some(title))
}

/// The Conversation's first user Message: the lowest-id one over all its Exchanges.
async fn first_user_message(conversation_id: i32, db: &DbConn) -> Result<Option<messages::Model>, SrvErr> {
    Ok(messages::Entity::find()
        .inner_join(exchanges::Entity)
        .filter(exchanges::Column::ConversationId.eq(conversation_id))
        .filter(messages::Column::Role.eq(MessageRole::User))
        .order_by_asc(messages::Column::Id)
        .one(db)
        .await?)
}

/// The title request for a Conversation whose first user Message reads `text`: no history, no tools.
fn request(text: &str) -> LlmRequest {
    LlmRequest {
        system: TITLE_PROMPT.to_owned(),
        messages: vec![LlmMessage {
            role: LlmRole::User,
            content: json!([{"type": "text", "text": format!("The first message of the conversation:\n\n{text}")}]),
        }],
        tools: Vec::new(),
    }
}

/// Why the title call gave no title.
enum NoTitle {
    /// The request failed: rejected, failed or idle mid-stream, or cut off. Recorded in `llm_calls.error`.
    Failed(LlmError),
    /// The request succeeded but the answer is no title (a refusal, or no text left once cleaned).
    /// Like an Exchange call that ends `Refused` or `EmptyResponse`, the call row gets no error.
    Unusable(&'static str),
}

impl From<LlmError> for NoTitle {
    fn from(error: LlmError) -> Self {
        NoTitle::Failed(error)
    }
}

/// Streams the answer to `request` into `accumulator` and returns its cleaned title. A request or
/// stream silent for `IDLE_TIMEOUT` fails like the Exchange's, and so does an unfinished stream.
async fn ask(request: LlmRequest, title_llm: &dyn Llm, accumulator: &mut Accumulator) -> Result<String, NoTitle> {
    // openrouter-rs sets no request timeout, so a provider that never answers would hang the job.
    let mut stream = match timeout(IDLE_TIMEOUT, title_llm.stream_message(request)).await {
        Err(_) => return Err(idle_error().into()),
        Ok(stream) => stream?,
    };
    // The stream may yield items after an error; the first error ends the call.
    loop {
        match timeout(IDLE_TIMEOUT, stream.next()).await {
            Err(_) => return Err(idle_error().into()),
            Ok(None) => break,
            Ok(Some(event)) => {
                accumulator.apply(event?);
            }
        }
    }
    if !accumulator.finished() {
        return Err(unfinished_error().into());
    }
    if accumulator.stop_reason() == Some(&StopReason::Refusal) {
        return Err(NoTitle::Unusable("the model refused to answer"));
    }
    clean_title(&answer_text(&accumulator.content())).ok_or(NoTitle::Unusable("the answer held no title"))
}

/// The text blocks of stored `content`, concatenated.
fn answer_text(content: &Value) -> String {
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect()
}

/// Completes the title call row `id` with what the stream reported and `error`.
async fn close_call(id: i32, accumulator: &Accumulator, error: Option<Value>, now: DateTime<FixedOffset>, db: &DbConn) -> Result<(), SrvErr> {
    let usage = accumulator.usage();
    llm_calls::Entity::update_many()
        .set(llm_calls::ActiveModel {
            stop_reason: Set(accumulator.stop_reason().map(|reason| reason.as_str().to_owned())),
            input_tokens: Set(tokens(usage.input_tokens)),
            output_tokens: Set(tokens(usage.output_tokens)),
            cache_read_tokens: Set(tokens(usage.cache_read_tokens)),
            cache_creation_tokens: Set(tokens(usage.cache_creation_tokens)),
            provider_request_id: Set(accumulator.request_id().map(str::to_owned)),
            error: Set(error),
            completed_at: Set(Some(now)),
            ..Default::default()
        })
        .filter(llm_calls::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(())
}

/// The model's answer as a title: whitespace runs collapsed to one space, trimmed, one layer of
/// matching surrounding quotes removed, trimmed again and cut to `MAX_TITLE_CHARS` chars. `None`
/// when nothing is left.
pub fn clean_title(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = unquoted(&collapsed).trim().chars().take(MAX_TITLE_CHARS).collect();
    let title = title.trim_end();
    (!title.is_empty()).then(|| title.to_owned())
}

/// `text` without one layer of matching surrounding quotes, if it has them.
fn unquoted(text: &str) -> &str {
    const QUOTES: [(char, char); 5] = [('"', '"'), ('\'', '\''), ('“', '”'), ('‘', '’'), ('`', '`')];
    QUOTES
        .iter()
        .find_map(|(open, close)| text.strip_prefix(*open)?.strip_suffix(*close))
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use std::panic::AssertUnwindSafe;

    use entities::conversations;
    use entities::sea_orm_active_enums::{LlmCallPurpose, MessageRole};
    use futures::FutureExt;
    use integrations::openrouter::{LlmEvent, LlmMessage, LlmRole, LlmUsage, StopReason};
    use sea_orm::{ActiveEnum, DatabaseConnection, Statement, Value};
    use serde_json::json;
    use tokio::sync::oneshot::error::RecvError;
    use super::*;
    use crate::chat::registry::ExchangeRegistry;
    use crate::test_support::{self, fixed_clock, MockLlm};
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, updated};
    use crate::test_support::fixtures::{conversation_model, message_model, title_llm_call_model};
    use crate::test_support::llm::{http_error, stalled_stream, stream_error, text_answer, thinking_answer, with_answer, with_error, with_recorded_answers, with_stream};

    #[test]
    fn clean_title_collapses_whitespace_when_title_has_newlines_and_runs_of_spaces() {
        test_support::init();
        assert_eq!(clean_title("  Alien \n  rewatch "), Some("Alien rewatch".to_string()));
    }

    #[test]
    fn clean_title_strips_quotes_when_title_is_quoted() {
        test_support::init();
        assert_eq!(clean_title("\"Alien\""), Some("Alien".to_string()));
        assert_eq!(clean_title("“Alien”"), Some("Alien".to_string()));
    }

    #[test]
    fn clean_title_returns_none_when_title_is_blank_or_only_quotes() {
        test_support::init();
        assert_eq!(clean_title("  "), None);
        assert_eq!(clean_title("\"\""), None);
    }

    #[test]
    fn clean_title_cuts_at_char_boundary_when_title_longer_than_80_chars() {
        test_support::init();
        assert_eq!(clean_title(&"é".repeat(100)), Some("é".repeat(80)));
    }

    /// Conversation 3 of user 1, whose first user Message (in Exchange 7) asks about Alien.
    const CONVERSATION: i32 = 3;

    /// The mock DB for a run that gets an answer: the Conversation, its first user Message, the
    /// inserted call row (id 11), then the update closing the call and the title update, which
    /// changes `title_rows` rows.
    fn answered_db(title_rows: u64) -> DatabaseConnection {
        started_db().append_exec_results([updated(1), updated(title_rows)]).into_connection()
    }

    /// The mock DB for a run whose call fails: as `answered_db`, without the title update.
    fn failed_db() -> DatabaseConnection {
        started_db().append_exec_results([updated(1)]).into_connection()
    }

    fn started_db() -> sea_orm::MockDatabase {
        test_support::db::mock()
            .append_query_results([[conversation_model(CONVERSATION, 1)]])
            .append_query_results([[message_model(1, 7, MessageRole::User, json!([{"type": "text", "text": "Shall we rewatch Alien?"}]))]])
            .append_query_results([[title_llm_call_model(11, CONVERSATION)]])
    }

    /// Titles Conversation 3 with its title channel open from the start; returns what the channel
    /// held right after the run (`None` when it is still open) and the statements executed.
    async fn run(db: DatabaseConnection, llm: &dyn Llm) -> (Option<Result<String, RecvError>>, Vec<Statement>) {
        let registry = ExchangeRegistry::new();
        let receiver = registry.open_title(CONVERSATION);

        generate_title(CONVERSATION, &db, llm, &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

        (receiver.now_or_never(), test_support::db::log(db))
    }

    fn closed(received: &Option<Result<String, RecvError>>) -> bool {
        matches!(received, Some(Err(_)))
    }

    /// The `llm_calls.error` of a call that failed with `message`, having no status nor response.
    fn call_error(message: &str) -> Value {
        Value::from(json!({"status": null, "message": message, "response": null}))
    }

    #[tokio::test]
    async fn generate_title_publishes_title_when_model_answers() {
        test_support::init();

        let (received, log) = run(answered_db(1), &with_answer(text_answer(&["\"Alien ", "\n rewatch\""], StopReason::EndTurn))).await;

        assert_eq!(received, Some(Ok("Alien rewatch".to_string())));
        assert_wrote(&log, r#"UPDATE "conversations""#, &[Value::from("Alien rewatch")]);
        assert_wrote(&log, r#"INSERT INTO "llm_calls""#, &[Value::from(LlmCallPurpose::Title.to_value())]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("end_turn"), Value::Json(None)]);
    }

    #[tokio::test]
    async fn generate_title_sends_first_message_with_title_prompt_when_called() {
        test_support::init();
        let (llm, requests) = with_recorded_answers(vec![text_answer(&["Alien rewatch"], StopReason::EndTurn)]);

        run(answered_db(1), &llm).await;

        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].system, TITLE_PROMPT);
        assert_eq!(requests[0].messages, vec![LlmMessage {
            role: LlmRole::User,
            content: json!([{"type": "text", "text": "The first message of the conversation:\n\nShall we rewatch Alien?"}]),
        }]);
        assert!(requests[0].tools.is_empty());
    }

    #[tokio::test]
    async fn generate_title_does_not_touch_updated_at_when_writing_title() {
        test_support::init();

        let (_, log) = run(answered_db(1), &with_answer(text_answer(&["Alien rewatch"], StopReason::EndTurn))).await;

        let expected = [Value::from("Alien rewatch"), Value::from(CONVERSATION)];
        assert_wrote(&log, r#"UPDATE "conversations""#, &expected);
        let update = log.iter().find(|statement| statement.sql.starts_with(r#"UPDATE "conversations""#)).expect("title update");
        assert_eq!(update.values.as_ref().map(|values| values.0.clone()), Some(expected.to_vec()));
    }

    #[tokio::test]
    async fn generate_title_publishes_nothing_when_provider_rejects_request() {
        test_support::init();

        let (received, log) = run(failed_db(), &with_error(http_error(400, "bad"))).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({"status": 400, "message": "bad", "response": null}))]);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
    }

    #[tokio::test]
    async fn generate_title_publishes_nothing_when_stream_fails_midway() {
        test_support::init();
        let mut events = text_answer(&["Ali"], StopReason::EndTurn);
        events.truncate(3);
        events.push(Err(stream_error("provider_overloaded", "overloaded")));

        let (received, log) = run(failed_db(), &with_answer(events)).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({
            "status": null,
            "message": "overloaded",
            "response": {"type": "api_error", "message": "overloaded", "error_type": "provider_overloaded"},
        }))]);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
    }

    #[tokio::test]
    async fn generate_title_publishes_nothing_when_answer_has_no_text() {
        test_support::init();

        let (received, log) = run(failed_db(), &with_answer(thinking_answer("hmm", StopReason::EndTurn))).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("end_turn"), Value::Json(None)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
    }

    #[tokio::test]
    async fn generate_title_publishes_nothing_when_model_refuses() {
        test_support::init();

        let (received, log) = run(failed_db(), &with_answer(text_answer(&["No"], StopReason::Refusal))).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("refusal"), Value::Json(None)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
    }

    #[tokio::test]
    async fn generate_title_publishes_nothing_when_stream_ends_unfinished() {
        test_support::init();
        let mut events = text_answer(&["Alien rewatch"], StopReason::EndTurn);
        events.truncate(3);

        let (received, log) = run(failed_db(), &with_answer(events)).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[call_error("stream ended before the answer finished")]);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
    }

    #[tokio::test(start_paused = true)]
    async fn generate_title_publishes_nothing_when_stream_is_idle_for_60_s() {
        test_support::init();
        let start = Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() });

        let (received, log) = run(failed_db(), &with_stream(stalled_stream(vec![start]))).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("gen-test"), call_error("no event for 60 s")]);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
    }

    #[tokio::test]
    async fn generate_title_publishes_nothing_when_conversation_deleted_before_title_written() {
        test_support::init();

        let (received, log) = run(answered_db(0), &with_answer(text_answer(&["Alien rewatch"], StopReason::EndTurn))).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("end_turn"), Value::Json(None)]);
        assert_wrote(&log, r#"UPDATE "conversations""#, &[Value::from("Alien rewatch")]);
    }

    #[tokio::test]
    async fn generate_title_calls_no_model_when_conversation_is_gone() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<conversations::Model>::new()]).into_connection();

        let (received, log) = run(db, &MockLlm::new()).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_statement_count(&log, 1);
    }

    #[tokio::test]
    async fn generate_title_calls_no_model_when_conversation_has_no_user_message() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(CONVERSATION, 1)]])
            .append_query_results([Vec::<entities::messages::Model>::new()])
            .into_connection();

        let (received, log) = run(db, &MockLlm::new()).await;

        assert!(closed(&received), "title channel not closed: {received:?}");
        assert_statement_count(&log, 2);
    }

    #[tokio::test]
    async fn generate_title_closes_title_channel_when_model_call_panics() {
        test_support::init();
        let db = started_db().into_connection();
        let mut llm = MockLlm::new();
        llm.expect_model().returning(|| "test/model".into());
        llm.expect_stream_message().returning(|_| panic!("stream_message panicked"));
        let registry = ExchangeRegistry::new();
        let receiver = registry.open_title(CONVERSATION);

        let result = AssertUnwindSafe(generate_title(CONVERSATION, &db, &llm, &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)))
            .catch_unwind()
            .await;

        assert!(result.is_err(), "the panic did not propagate");
        let received = receiver.now_or_never();
        assert!(closed(&received), "title channel not closed after the panic: {received:?}");
    }

    #[tokio::test]
    async fn generate_title_leaves_no_entry_behind_when_receiver_dropped() {
        test_support::init();
        let db = answered_db(1);
        let registry = ExchangeRegistry::new();
        drop(registry.open_title(CONVERSATION));

        generate_title(CONVERSATION, &db, &with_answer(text_answer(&["Alien rewatch"], StopReason::EndTurn)), &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

        registry.publish_title(CONVERSATION, "Later".into());
        registry.close_title(CONVERSATION);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "conversations""#, &[Value::from("Alien rewatch")]);
    }
}
