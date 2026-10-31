//! Running an Exchange: one streamed LLM call whose answer is written to the database chunk by
//! chunk and forwarded to subscribers only after each write commits.

use std::time::Duration;

use chrono::{DateTime, FixedOffset};
use entities::sea_orm_active_enums::{ExchangeStatus, LlmCallPurpose, MessageRole};
use entities::{conversations, exchanges, llm_calls, messages};
use futures::{FutureExt, StreamExt};
use integrations::openrouter::{Llm, LlmError, LlmEventStream, StopReason};
use log::{error, warn};
use sea_orm::sea_query::Query;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, DbConn, EntityTrait, QueryFilter, QueryOrder, TransactionTrait};
use serde_json::{json, Value};
use tokio::time::timeout;
use views::conversations::ExchangeEvent;

use crate::chat::accumulator::Accumulator;
use crate::chat::display::display_blocks;
use crate::chat::failure::{classify, debug_message, exchange_error, llm_call_error, ExchangeFailure};
use crate::chat::history::build_request;
use crate::chat::registry::ExchangeRegistry;
use crate::infrastructure::{Clock, SrvErr};

/// How long the model stream may stay silent before the call counts as failed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Streams the model's answer for Exchange `exchange_id`, stores it and ends the Exchange, then
/// publishes the terminal event and closes the Exchange's registry entry.
///
/// Every failure, ours included, ends the Exchange as failed and returns `Ok`. When the Exchange
/// row disappears (its Conversation was deleted) the run stops quietly and publishes nothing.
/// Returns `Err` only when even recording the failure fails.
pub async fn run_exchange(exchange_id: i32, db: &DbConn, llm: &dyn Llm, registry: &ExchangeRegistry, clock: &dyn Clock) -> Result<(), SrvErr> {
    let _close = CloseOnDrop { registry, exchange_id };
    let mut run = Run::new(exchange_id);
    let end = match run.call(db, llm, registry, clock).await {
        Ok(end) => end,
        Err(err) => {
            error!("Exchange {exchange_id} failed on our side, marking it failed: {err}");
            End::internal(&err)
        }
    };
    let end = match run.settle(end, db, clock).await {
        Ok(end) => end,
        Err(err) => {
            error!("Ending Exchange {exchange_id} failed, marking it failed instead: {err}");
            run.settle(End::internal(&err), db, clock).await?
        }
    };
    match end {
        End::Completed => registry.publish(exchange_id, ExchangeEvent::Completed { exchange_id }),
        End::Failed { failure, debug_message, .. } => {
            registry.publish(exchange_id, ExchangeEvent::Failed { exchange_id, key: failure.to_string(), debug_message })
        }
        End::Deleted => {}
    }
    Ok(())
}

/// Closes the Exchange's registry entry when dropped, so the streams following the Exchange end
/// even when its run returns early or panics. Closing an entry that is already gone does nothing.
struct CloseOnDrop<'a> {
    registry: &'a ExchangeRegistry,
    exchange_id: i32,
}

impl Drop for CloseOnDrop<'_> {
    fn drop(&mut self) {
        self.registry.close(self.exchange_id);
    }
}

/// Marks every in-progress Exchange as failed `Interrupted` and closes their open LLM calls.
/// Returns how many Exchanges it marked. Run at startup, before any Exchange can be running.
pub async fn recover_interrupted(db: &DbConn, clock: &dyn Clock) -> Result<u64, SrvErr> {
    let now = clock.now().fixed_offset();
    let error = error_json(ExchangeFailure::Interrupted, "server stopped while the exchange was running".into())?;
    let in_progress = Query::select()
        .column(exchanges::Column::Id)
        .from(exchanges::Entity)
        .and_where(exchanges::Column::Status.eq(ExchangeStatus::InProgress))
        .to_owned();
    let tran = db.begin().await?;
    llm_calls::Entity::update_many()
        .set(llm_calls::ActiveModel {
            error: Set(Some(json!({"message": "interrupted"}))),
            completed_at: Set(Some(now)),
            ..Default::default()
        })
        .filter(llm_calls::Column::CompletedAt.is_null())
        .filter(llm_calls::Column::ExchangeId.in_subquery(in_progress))
        .exec(&tran)
        .await?;
    let marked = exchanges::Entity::update_many()
        .set(exchanges::ActiveModel {
            status: Set(ExchangeStatus::Failed),
            error: Set(Some(error)),
            completed_at: Set(Some(now)),
            ..Default::default()
        })
        .filter(exchanges::Column::Status.eq(ExchangeStatus::InProgress))
        .exec(&tran)
        .await?;
    tran.commit().await?;
    Ok(marked.rows_affected)
}

/// How an Exchange ends.
enum End {
    Completed,
    Failed {
        failure: ExchangeFailure,
        debug_message: String,
        /// For `llm_calls.error`; `None` when the call itself went fine.
        call_error: Option<Value>,
    },
    /// The Exchange row is gone because its Conversation was deleted.
    Deleted,
}

impl End {
    fn provider(error: &LlmError) -> Self {
        End::Failed { failure: classify(error), debug_message: debug_message(error), call_error: Some(llm_call_error(error)) }
    }

    fn internal(error: &SrvErr) -> Self {
        let message = error.to_string();
        End::Failed { failure: ExchangeFailure::Internal, debug_message: message.clone(), call_error: Some(json!({"message": message})) }
    }

    fn failed(failure: ExchangeFailure, debug_message: &str) -> Self {
        End::Failed { failure, debug_message: debug_message.into(), call_error: None }
    }
}

/// Why reading the model stream stopped.
enum Stop {
    Ended,
    Failed(LlmError),
}

/// The state of one run.
struct Run {
    exchange_id: i32,
    llm_call_id: Option<i32>,
    message_id: Option<i32>,
    accumulator: Accumulator,
    /// The content last written to the assistant Message.
    written: Value,
}

impl Run {
    fn new(exchange_id: i32) -> Self {
        Self { exchange_id, llm_call_id: None, message_id: None, accumulator: Accumulator::new(0), written: Value::Array(Vec::new()) }
    }

    /// Makes the LLM call and stores its answer as it streams in. Returns how the Exchange ends.
    async fn call(&mut self, db: &DbConn, llm: &dyn Llm, registry: &ExchangeRegistry, clock: &dyn Clock) -> Result<End, SrvErr> {
        let Some(exchange) = exchanges::Entity::find_by_id(self.exchange_id).one(db).await? else {
            return Ok(End::Deleted);
        };
        let conversation = conversations::Entity::find_by_id(exchange.conversation_id).one(db).await?.ok_or(SrvErr::NotFound)?;
        let history = load_history(conversation.id, db).await?;
        let current = history.iter().find(|(exchange, _)| exchange.id == self.exchange_id).map(|(_, messages)| messages.as_slice());
        self.accumulator = Accumulator::new(display_blocks(current.unwrap_or_default()).len() as u32);
        let request = build_request(self.exchange_id, &history);

        let call = llm_calls::ActiveModel {
            user_id: Set(Some(conversation.user_id)),
            conversation_id: Set(Some(conversation.id)),
            exchange_id: Set(Some(self.exchange_id)),
            purpose: Set(LlmCallPurpose::Exchange),
            model: Set(llm.model()),
            created_at: Set(clock.now().fixed_offset()),
            ..Default::default()
        }
        .insert(db)
        .await?;
        self.llm_call_id = Some(call.id);

        // openrouter-rs sets no request timeout, so a provider that never answers would hang the run.
        let stream = match timeout(IDLE_TIMEOUT, llm.stream_message(request)).await {
            Err(_) => return Ok(End::provider(&idle_error())),
            Ok(Err(error)) => return Ok(End::provider(&error)),
            Ok(Ok(stream)) => stream,
        };
        Ok(match self.read(stream, db, registry, clock).await? {
            None => End::Deleted,
            Some(Stop::Failed(error)) => End::provider(&error),
            Some(Stop::Ended) => self.outcome(),
        })
    }

    /// Reads `stream` until it ends, fails or stays idle for `IDLE_TIMEOUT`. Each event is applied
    /// together with every event already waiting behind it; their content is written, then their
    /// deltas are published, one per block. `None` when the Exchange row is gone.
    async fn read(&mut self, mut stream: LlmEventStream, db: &DbConn, registry: &ExchangeRegistry, clock: &dyn Clock) -> Result<Option<Stop>, SrvErr> {
        loop {
            let mut deltas = match timeout(IDLE_TIMEOUT, stream.next()).await {
                Err(_) => return Ok(Some(Stop::Failed(idle_error()))),
                Ok(None) => return Ok(Some(Stop::Ended)),
                Ok(Some(Err(error))) => return Ok(Some(Stop::Failed(error))),
                Ok(Some(Ok(event))) => self.accumulator.apply(event),
            };
            // The stream may yield items after an error; the first error ends the call.
            let stop = loop {
                match stream.next().now_or_never() {
                    None => break None,
                    Some(None) => break Some(Stop::Ended),
                    Some(Some(Err(error))) => break Some(Stop::Failed(error)),
                    Some(Some(Ok(event))) => deltas.extend(self.accumulator.apply(event)),
                }
            };
            if !self.write(db, clock).await? {
                return Ok(None);
            }
            for delta in merge(deltas) {
                registry.publish(self.exchange_id, delta);
            }
            if stop.is_some() {
                return Ok(stop);
            }
        }
    }

    /// Writes the accumulated content to the assistant Message if it changed, creating the
    /// Message on the first content. `false` when the Exchange row is gone.
    async fn write(&mut self, db: &DbConn, clock: &dyn Clock) -> Result<bool, SrvErr> {
        let content = self.accumulator.content();
        if !self.accumulator.has_content() || content == self.written {
            return Ok(true);
        }
        let written = match self.message_id {
            None => messages::ActiveModel {
                exchange_id: Set(self.exchange_id),
                role: Set(MessageRole::Assistant),
                content: Set(content.clone()),
                llm_call_id: Set(self.llm_call_id),
                created_at: Set(clock.now().fixed_offset()),
                ..Default::default()
            }
            .insert(db)
            .await
            .map(|message| {
                self.message_id = Some(message.id);
                true
            }),
            Some(id) => messages::Entity::update_many()
                .set(messages::ActiveModel { content: Set(content.clone()), ..Default::default() })
                .filter(messages::Column::Id.eq(id))
                .exec(db)
                .await
                .map(|result| result.rows_affected > 0),
        };
        if let Ok(true) = written {
            self.written = content;
            return Ok(true);
        }
        if !exchange_exists(self.exchange_id, db).await? {
            return Ok(false);
        }
        match written {
            Err(err) => Err(err.into()),
            Ok(_) => Err(SrvErr::Internal(format!("assistant message of Exchange {} disappeared", self.exchange_id))),
        }
    }

    /// How a call whose stream ended without error ends the Exchange, by its stop reason. A stream
    /// that ended before the model finished (no stop reason and no `MessageStop`) was cut off.
    fn outcome(&self) -> End {
        if !self.accumulator.finished() {
            return End::provider(&unfinished_error());
        }
        match self.accumulator.stop_reason() {
            Some(StopReason::Refusal) => End::failed(ExchangeFailure::Refused, "the model refused to answer"),
            Some(StopReason::ToolUse) => End::failed(ExchangeFailure::Internal, "the model asked to use a tool, but none are offered"),
            _ if !self.accumulator.has_text() => End::failed(ExchangeFailure::EmptyResponse, "the model ended its answer without text"),
            Some(StopReason::EndTurn | StopReason::StopSequence | StopReason::MaxTokens) => End::Completed,
            Some(StopReason::Other(reason)) => {
                warn!("Exchange {} ended with unknown stop reason {reason:?}; completing it", self.exchange_id);
                End::Completed
            }
            None => {
                warn!("Exchange {} ended with message_stop but without a stop reason; completing it", self.exchange_id);
                End::Completed
            }
        }
    }

    /// Records `end`: the LLM call and the Exchange in one transaction. Returns what to publish,
    /// which is `Deleted` when the Exchange row turns out to be gone.
    async fn settle(&self, end: End, db: &DbConn, clock: &dyn Clock) -> Result<End, SrvErr> {
        let now = clock.now().fixed_offset();
        let (status, error, call_error) = match &end {
            End::Deleted => {
                self.close_call(Some(deleted_error()), now, db).await?;
                return Ok(End::Deleted);
            }
            End::Completed => (ExchangeStatus::Completed, None, None),
            End::Failed { failure, debug_message, call_error } => {
                (ExchangeStatus::Failed, Some(error_json(*failure, debug_message.clone())?), call_error.clone())
            }
        };
        let tran = db.begin().await?;
        let updated = exchanges::Entity::update_many()
            .set(exchanges::ActiveModel { status: Set(status), error: Set(error), completed_at: Set(Some(now)), ..Default::default() })
            .filter(exchanges::Column::Id.eq(self.exchange_id))
            .exec(&tran)
            .await?;
        if updated.rows_affected == 0 {
            tran.rollback().await?;
            self.close_call(Some(deleted_error()), now, db).await?;
            return Ok(End::Deleted);
        }
        self.close_call(call_error, now, &tran).await?;
        tran.commit().await?;
        Ok(end)
    }

    /// Completes the LLM call row, if one was inserted, with what the stream reported and `error`.
    async fn close_call<C: ConnectionTrait>(&self, error: Option<Value>, now: DateTime<FixedOffset>, conn: &C) -> Result<(), SrvErr> {
        let Some(id) = self.llm_call_id else { return Ok(()) };
        let usage = self.accumulator.usage();
        llm_calls::Entity::update_many()
            .set(llm_calls::ActiveModel {
                stop_reason: Set(self.accumulator.stop_reason().map(|reason| reason.as_str().to_owned())),
                input_tokens: Set(tokens(usage.input_tokens)),
                output_tokens: Set(tokens(usage.output_tokens)),
                cache_read_tokens: Set(tokens(usage.cache_read_tokens)),
                cache_creation_tokens: Set(tokens(usage.cache_creation_tokens)),
                provider_request_id: Set(self.accumulator.request_id().map(str::to_owned)),
                error: Set(error),
                completed_at: Set(Some(now)),
                ..Default::default()
            })
            .filter(llm_calls::Column::Id.eq(id))
            .exec(conn)
            .await?;
        Ok(())
    }
}

/// The Conversation's Exchanges ordered by id, each with its Messages ordered by id.
async fn load_history(conversation_id: i32, db: &DbConn) -> Result<Vec<(exchanges::Model, Vec<messages::Model>)>, SrvErr> {
    let exchanges = exchanges::Entity::find()
        .filter(exchanges::Column::ConversationId.eq(conversation_id))
        .order_by_asc(exchanges::Column::Id)
        .all(db)
        .await?;
    let messages = messages::Entity::find()
        .filter(messages::Column::ExchangeId.is_in(exchanges.iter().map(|exchange| exchange.id)))
        .order_by_asc(messages::Column::Id)
        .all(db)
        .await?;
    Ok(exchanges
        .into_iter()
        .map(|exchange| {
            let own = messages.iter().filter(|message| message.exchange_id == exchange.id).cloned().collect();
            (exchange, own)
        })
        .collect())
}

async fn exchange_exists(exchange_id: i32, db: &DbConn) -> Result<bool, SrvErr> {
    Ok(exchanges::Entity::find_by_id(exchange_id).one(db).await?.is_some())
}

/// Joins the deltas of each block, in the order the blocks first appear. A block has one kind,
/// so its number identifies it.
fn merge(deltas: Vec<ExchangeEvent>) -> Vec<ExchangeEvent> {
    let mut merged: Vec<ExchangeEvent> = Vec::new();
    for delta in deltas {
        let Some((block, text)) = delta_parts(&delta) else { continue };
        match merged.iter_mut().find(|merged| delta_parts(merged).is_some_and(|(merged_block, _)| merged_block == block)) {
            Some(ExchangeEvent::TextDelta { text: joined, .. } | ExchangeEvent::ThinkingDelta { text: joined, .. }) => joined.push_str(text),
            _ => merged.push(delta),
        }
    }
    merged
}

/// The block and text of a `TextDelta` or `ThinkingDelta`.
fn delta_parts(event: &ExchangeEvent) -> Option<(u32, &str)> {
    match event {
        ExchangeEvent::TextDelta { block, text } | ExchangeEvent::ThinkingDelta { block, text } => Some((*block, text)),
        _ => None,
    }
}

/// The error of a request or stream that stayed silent for `IDLE_TIMEOUT`. Having neither status
/// nor error type, it classifies as `ProviderUnavailable`.
fn idle_error() -> LlmError {
    LlmError { status: None, error_type: None, message: format!("no event for {} s", IDLE_TIMEOUT.as_secs()), response: None }
}

/// The error of a stream that ended without error before the model finished its message. Having
/// neither status nor error type, it classifies as `ProviderUnavailable`.
fn unfinished_error() -> LlmError {
    LlmError { status: None, error_type: None, message: "stream ended before the answer finished".into(), response: None }
}

/// A token count for an `i32` column; counts beyond its range are stored as `i32::MAX`.
fn tokens(count: Option<u64>) -> Option<i32> {
    count.map(|count| i32::try_from(count).unwrap_or(i32::MAX))
}

fn deleted_error() -> Value {
    json!({"message": "conversation deleted"})
}

/// `exchanges.error` for `failure`.
fn error_json(failure: ExchangeFailure, debug_message: String) -> Result<Value, SrvErr> {
    serde_json::to_value(exchange_error(failure, debug_message)).map_err(|err| SrvErr::Internal(err.to_string()))
}

#[cfg(test)]
mod tests {
    use entities::sea_orm_active_enums::{ExchangeStatus, MessageRole};
    use integrations::openrouter::{LlmEvent, LlmUsage, StopReason};
    use sea_orm::{DatabaseConnection, Statement, Value};
    use serde_json::json;
    use views::conversations::ExchangeEvent;
    use super::*;
    use crate::test_support::{self, fixed_clock, MockLlm};
    use crate::test_support::db::{assert_no_statement_starting_with, assert_wrote, updated, with_started_exchange_run};
    use crate::test_support::fixtures::message_model;
    use crate::test_support::llm::{
        http_error, panicking_stream, stalled_stream, stream_error, text_answer, thinking_answer, thinking_then_text_answer, trickled_stream, with_answer, with_error, with_stream,
        UnansweringLlm,
    };

    /// Runs Exchange 7 with a subscriber attached from the start; returns whether the run
    /// returned `Ok`, the events the subscriber received, and the statements executed.
    async fn run(db: DatabaseConnection, llm: MockLlm) -> (bool, Vec<ExchangeEvent>, Vec<Statement>) {
        let registry = ExchangeRegistry::new();
        registry.open(7);
        let mut live = registry.subscribe(7).expect("open").live.expect("live");

        let result = run_exchange(7, &db, &llm, &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

        let mut events = Vec::new();
        while let Some(event) = live.recv().await {
            events.push(event);
        }
        (result.is_ok(), events, test_support::db::log(db))
    }

    /// The mock DB for a run that writes one assistant Message and ends the Exchange.
    fn answering_db() -> DatabaseConnection {
        with_started_exchange_run(test_support::db::mock(), 7)
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1), updated(1)])
            .into_connection()
    }

    /// The mock DB for a run that writes no Message and ends the Exchange.
    fn silent_db() -> DatabaseConnection {
        with_started_exchange_run(test_support::db::mock(), 7)
            .append_exec_results([updated(1), updated(1)])
            .into_connection()
    }

    fn delta(block: u32, text: &str) -> ExchangeEvent {
        ExchangeEvent::TextDelta { block, text: text.into() }
    }

    fn thinking_delta(block: u32, text: &str) -> ExchangeEvent {
        ExchangeEvent::ThinkingDelta { block, text: text.into() }
    }

    fn failed(key: &str, debug_message: &str) -> ExchangeEvent {
        ExchangeEvent::Failed { exchange_id: 7, key: key.into(), debug_message: debug_message.into() }
    }

    fn status(status: ExchangeStatus) -> Value {
        Value::from(sea_orm::ActiveEnum::to_value(&status))
    }

    fn statements_starting_with(log: &[Statement], prefix: &str) -> usize {
        log.iter().filter(|statement| statement.sql.starts_with(prefix)).count()
    }

    #[tokio::test]
    async fn run_exchange_stores_answer_and_completes_when_stream_ends() {
        test_support::init();

        let (ok, events, log) = run(answering_db(), with_answer(text_answer(&["Hello"], StopReason::EndTurn))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hello"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "text", "text": "Hello"}]))]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_coalesces_ready_deltas_into_one_write_and_one_event_when_stream_is_buffered() {
        test_support::init();

        let (ok, events, log) = run(answering_db(), with_answer(text_answer(&["Hel", "lo", " world"], StopReason::EndTurn))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hello world"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 1);
        assert_no_statement_starting_with(&log, r#"UPDATE "messages""#);
    }

    #[tokio::test]
    async fn run_exchange_inserts_then_updates_message_when_chunks_arrive_separately() {
        test_support::init();
        let db = with_started_exchange_run(test_support::db::mock(), 7)
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1), updated(1), updated(1)])
            .into_connection();

        let (ok, events, log) = run(db, with_stream(trickled_stream(text_answer(&["Hel", "lo"], StopReason::EndTurn)))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hel"), delta(0, "lo"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 1);
        assert_eq!(statements_starting_with(&log, r#"UPDATE "messages""#), 1);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "text", "text": "Hel"}]))]);
        assert_wrote(&log, r#"UPDATE "messages""#, &[Value::from(json!([{"type": "text", "text": "Hello"}]))]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_fails_with_unavailable_and_keeps_partial_message_when_stream_ends_unfinished() {
        test_support::init();
        let mut events = text_answer(&["Hel"], StopReason::EndTurn);
        events.truncate(3);

        let (ok, events, log) = run(answering_db(), with_answer(events)).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hel"), failed("ProviderUnavailable", "stream ended before the answer finished")]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "text", "text": "Hel"}]))]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(
            json!({"status": null, "message": "stream ended before the answer finished", "response": null}),
        )]);
    }

    #[tokio::test]
    async fn run_exchange_completes_when_message_stop_arrived_without_stop_reason() {
        test_support::init();
        let events = text_answer(&["Hello"], StopReason::EndTurn)
            .into_iter()
            .filter(|event| !matches!(event, Ok(LlmEvent::MessageDelta { .. })))
            .collect();

        let (ok, events, _) = run(answering_db(), with_answer(events)).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hello"), ExchangeEvent::Completed { exchange_id: 7 }]);
    }

    #[tokio::test]
    async fn run_exchange_closes_registry_entry_when_run_panics() {
        test_support::init();
        let db = with_started_exchange_run(test_support::db::mock(), 7).into_connection();
        let llm = with_stream(panicking_stream());
        let registry = ExchangeRegistry::new();
        registry.open(7);
        let mut live = registry.subscribe(7).expect("open").live.expect("live");

        let result = std::panic::AssertUnwindSafe(run_exchange(7, &db, &llm, &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)))
            .catch_unwind()
            .await;

        assert!(result.is_err());
        let ended = tokio::time::timeout(Duration::from_secs(5), live.recv()).await;
        assert_eq!(ended, Ok(None), "live stream still open after the run panicked");
        assert!(registry.subscribe(7).is_none());
    }

    #[tokio::test]
    async fn run_exchange_completes_when_subscriber_dropped() {
        test_support::init();
        let db = answering_db();
        let llm = with_answer(text_answer(&["Hello"], StopReason::EndTurn));
        let registry = ExchangeRegistry::new();
        registry.open(7);
        drop(registry.subscribe(7));

        let result = run_exchange(7, &db, &llm, &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

        assert!(result.is_ok());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_keeps_partial_message_when_stream_fails_mid_answer() {
        test_support::init();
        let mut events = text_answer(&["Hel"], StopReason::EndTurn);
        events.truncate(3);
        events.push(Err(stream_error("provider_overloaded", "overloaded")));
        events.push(Ok(LlmEvent::TextDelta { index: 0, text: "lo".into() }));

        let (ok, events, log) = run(answering_db(), with_answer(events)).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hel"), failed("ProviderUnavailable", "provider_overloaded: overloaded")]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "text", "text": "Hel"}]))]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
    }

    #[tokio::test]
    async fn run_exchange_stores_no_message_when_request_fails_before_streaming() {
        test_support::init();

        let (ok, events, log) = run(silent_db(), with_error(http_error(400, "bad"))).await;

        assert!(ok);
        assert_eq!(events, vec![failed("ProviderError", "400: bad")]);
        assert_no_statement_starting_with(&log, r#"INSERT INTO "messages""#);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[Value::from(json!({"key": "ProviderError", "debug_message": "400: bad"}))]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({"status": 400, "message": "bad", "response": null}))]);
    }

    #[tokio::test]
    async fn run_exchange_fails_with_empty_response_when_only_thinking_arrived() {
        test_support::init();

        let (ok, events, log) = run(answering_db(), with_answer(thinking_answer("hmm", StopReason::EndTurn))).await;

        assert!(ok);
        assert_eq!(events, vec![thinking_delta(0, "hmm"), failed("EmptyResponse", "the model ended its answer without text")]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "thinking", "thinking": "hmm", "signature": ""}]))]);
    }

    #[tokio::test]
    async fn run_exchange_publishes_thinking_then_text_in_block_order_when_buffered() {
        test_support::init();

        let (ok, events, log) = run(answering_db(), with_answer(thinking_then_text_answer("hmm", &["Hi"], StopReason::EndTurn))).await;

        assert!(ok);
        assert_eq!(events, vec![thinking_delta(0, "hmm"), delta(1, "Hi"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 1);
        assert_wrote(
            &log,
            r#"INSERT INTO "messages""#,
            &[Value::from(json!([{"type": "thinking", "thinking": "hmm", "signature": ""}, {"type": "text", "text": "Hi"}]))],
        );
    }

    #[tokio::test]
    async fn run_exchange_keeps_thinking_as_partial_message_when_stream_fails_mid_thinking() {
        test_support::init();
        let mut events = thinking_answer("hmm", StopReason::EndTurn);
        events.truncate(3);
        events.push(Err(stream_error("provider_overloaded", "overloaded")));

        let (ok, events, log) = run(answering_db(), with_answer(events)).await;

        assert!(ok);
        assert_eq!(events, vec![thinking_delta(0, "hmm"), failed("ProviderUnavailable", "provider_overloaded: overloaded")]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "thinking", "thinking": "hmm", "signature": ""}]))]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
    }

    #[tokio::test]
    async fn run_exchange_fails_with_refused_when_stop_reason_is_refusal() {
        test_support::init();

        let (ok, events, _) = run(answering_db(), with_answer(text_answer(&["No"], StopReason::Refusal))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "No"), failed("Refused", "the model refused to answer")]);
    }

    #[tokio::test]
    async fn run_exchange_completes_when_stop_reason_is_max_tokens() {
        test_support::init();

        let (ok, events, _) = run(answering_db(), with_answer(text_answer(&["Hel"], StopReason::MaxTokens))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hel"), ExchangeEvent::Completed { exchange_id: 7 }]);
    }

    #[tokio::test(start_paused = true)]
    async fn run_exchange_fails_with_unavailable_when_stream_goes_idle() {
        test_support::init();
        let start = Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() });

        let (ok, events, log) = run(silent_db(), with_stream(stalled_stream(vec![start]))).await;

        assert!(ok);
        assert_eq!(events, vec![failed("ProviderUnavailable", "no event for 60 s")]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({"status": null, "message": "no event for 60 s", "response": null}))]);
    }

    #[tokio::test(start_paused = true)]
    async fn run_exchange_fails_with_unavailable_when_request_never_answers() {
        test_support::init();
        let db = silent_db();
        let registry = ExchangeRegistry::new();
        registry.open(7);
        let mut live = registry.subscribe(7).expect("open").live.expect("live");

        let result = run_exchange(7, &db, &UnansweringLlm, &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

        assert!(result.is_ok());
        assert_eq!(live.recv().await, Some(failed("ProviderUnavailable", "no event for 60 s")));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({"status": null, "message": "no event for 60 s", "response": null}))]);
    }

    #[tokio::test]
    async fn recover_interrupted_marks_in_progress_exchanges_failed() {
        test_support::init();
        let db = test_support::db::mock()
            .append_exec_results([updated(1), updated(2)])
            .into_connection();

        let marked = recover_interrupted(&db, &fixed_clock(2024, 1, 1, 12, 0, 0)).await.ok().expect("recovered");

        assert_eq!(marked, 2);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[
            status(ExchangeStatus::Failed),
            Value::from(json!({"key": "Interrupted", "debug_message": "server stopped while the exchange was running"})),
        ]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({"message": "interrupted"}))]);
    }
}
