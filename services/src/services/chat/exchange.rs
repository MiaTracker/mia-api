//! Running an Exchange: streamed LLM calls whose answers are written to the database chunk by
//! chunk and forwarded to subscribers only after each write commits. A call that asks for tools is
//! followed by a tool round and another call, until the model answers.

use std::time::Duration;

use chrono::{DateTime, FixedOffset};
use entities::sea_orm_active_enums::{ExchangeStatus, LlmCallPurpose, MessageRole};
use entities::{conversations, exchanges, llm_calls, messages, users};
use futures::{FutureExt, StreamExt};
use infrastructure::config;
use integrations::openrouter::{Llm, LlmError, LlmEventStream, StopReason};
use log::{error, warn};
use sea_orm::sea_query::Query;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, DbConn, EntityTrait, QueryFilter, QueryOrder, TransactionTrait};
use serde_json::{json, Value};
use tokio::time::timeout;
use views::conversations::ExchangeEvent;
use views::users::CurrentUser;

use crate::chat::accumulator::Accumulator;
use crate::chat::display::display_blocks;
use crate::chat::failure::{classify, debug_message, exchange_error, llm_call_error, ExchangeFailure};
use crate::chat::history::build_request;
use crate::chat::registry::ExchangeRegistry;
use crate::chat::tools::{failed, result_block, run_tool};
use crate::infrastructure::{Clock, SrvErr, Tmdb};

/// How long the model stream may stay silent before the call counts as failed.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// How long one tool call may run before it counts as failed. The TMDB client sets no request
/// timeout and honours any `Retry-After`, so a stalled tool would otherwise hang the Exchange.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// The result given for each tool call refused at the tool round limit.
const ROUND_LIMIT_RESULT: &str = "tool limit reached; answer with what you have";

/// Streams the model's answer for Exchange `exchange_id`, running the tools it asks for in between
/// calls, stores it and ends the Exchange, then publishes the terminal event and closes the
/// Exchange's registry entry.
///
/// Every failure, ours included, ends the Exchange as failed and returns `Ok`. When the Exchange
/// row disappears (its Conversation was deleted) the run stops quietly and publishes nothing.
/// Returns `Err` only when even recording the failure fails.
pub async fn run_exchange(exchange_id: i32, db: &DbConn, llm: &dyn Llm, registry: &ExchangeRegistry, tmdb: &dyn Tmdb, clock: &dyn Clock) -> Result<(), SrvErr> {
    let _close = CloseOnDrop { registry, exchange_id };
    let mut run = Run::new(exchange_id);
    let end = match run.exchange(db, llm, registry, tmdb, clock).await {
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

/// Marks every in-progress Exchange as failed `Interrupted` and closes their open LLM calls, and
/// closes every open title call the same way (`{"message": "interrupted"}`). Returns how many
/// Exchanges it marked. Run at startup, before any job can be running: a title call still open
/// then belongs to a title job the stop killed.
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
        .filter(
            Condition::any()
                .add(llm_calls::Column::ExchangeId.in_subquery(in_progress))
                .add(llm_calls::Column::Purpose.eq(LlmCallPurpose::Title)),
        )
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

/// What follows an LLM call.
enum Next {
    End(End),
    /// The model asked for tools: run a tool round, then call it again.
    ToolRound,
}

/// Why reading the model stream stopped.
enum Stop {
    Ended,
    Failed(LlmError),
}

/// The Conversation's Exchanges ordered by creation time then id, each with its Messages ordered by id.
type History = Vec<(exchanges::Model, Vec<messages::Model>)>;

/// The state of one run. The call fields describe the latest LLM call.
struct Run {
    exchange_id: i32,
    llm_call_id: Option<i32>,
    /// The call's assistant Message, once created.
    message: Option<messages::Model>,
    accumulator: Accumulator,
    /// The content last written to the call's assistant Message.
    written: Value,
    /// How many tool rounds ran their tools.
    rounds: u32,
    /// Whether the last tool round's calls were refused at the round limit.
    refused: bool,
    /// The Conversation's user, loaded on the first tool round.
    user: Option<CurrentUser>,
}

impl Run {
    fn new(exchange_id: i32) -> Self {
        Self {
            exchange_id,
            llm_call_id: None,
            message: None,
            accumulator: Accumulator::new(0),
            written: Value::Array(Vec::new()),
            rounds: 0,
            refused: false,
            user: None,
        }
    }

    /// Calls the model until it answers, with a tool round after every call that asks for tools.
    /// The history is loaded once; the Messages this run stores are added to it as they are written.
    /// Returns how the Exchange ends.
    async fn exchange(&mut self, db: &DbConn, llm: &dyn Llm, registry: &ExchangeRegistry, tmdb: &dyn Tmdb, clock: &dyn Clock) -> Result<End, SrvErr> {
        let Some(exchange) = exchanges::Entity::find_by_id(self.exchange_id).one(db).await? else {
            return Ok(End::Deleted);
        };
        let conversation = conversations::Entity::find_by_id(exchange.conversation_id).one(db).await?.ok_or(SrvErr::NotFound)?;
        let mut history = load_history(conversation.id, db).await?;
        let Some(current) = history.iter().position(|(exchange, _)| exchange.id == self.exchange_id) else {
            return Ok(End::Deleted);
        };
        (self.rounds, self.refused) = spent_rounds(&history[current].1, config().assistant.max_tool_rounds);
        loop {
            if let Next::End(end) = self.call(&conversation, &history, current, db, llm, registry, clock).await? {
                return Ok(end);
            }
            if let Some(end) = self.tool_round(conversation.user_id, &mut history[current].1, db, registry, tmdb, clock).await? {
                return Ok(end);
            }
        }
    }

    /// Makes one LLM call over `history`, whose entry `current` is this Exchange, and stores its
    /// answer as it streams in. Returns what follows.
    #[allow(clippy::too_many_arguments)]
    async fn call(
        &mut self,
        conversation: &conversations::Model,
        history: &History,
        current: usize,
        db: &DbConn,
        llm: &dyn Llm,
        registry: &ExchangeRegistry,
        clock: &dyn Clock,
    ) -> Result<Next, SrvErr> {
        self.llm_call_id = None;
        self.message = None;
        self.written = Value::Array(Vec::new());
        self.accumulator = Accumulator::new(display_blocks(&history[current].1).len() as u32);
        let request = build_request(self.exchange_id, history);

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
            Err(_) => return Ok(Next::End(End::provider(&idle_error()))),
            Ok(Err(error)) => return Ok(Next::End(End::provider(&error))),
            Ok(Ok(stream)) => stream,
        };
        Ok(match self.read(stream, db, registry, clock).await? {
            None => Next::End(End::Deleted),
            Some(Stop::Failed(error)) => Next::End(End::provider(&error)),
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
        let written = match &self.message {
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
                self.message = Some(message);
                true
            }),
            Some(message) => messages::Entity::update_many()
                .set(messages::ActiveModel { content: Set(content.clone()), ..Default::default() })
                .filter(messages::Column::Id.eq(message.id))
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

    /// What follows a call whose stream ended without error: decided by its content first and its
    /// stop reason second, since providers disagree on the stop reason when tools are involved. A
    /// stream that ended before the model finished (no stop reason and no `MessageStop`) was cut
    /// off. Any tool_use block makes a tool round; without one, `ToolUse` is judged like `EndTurn`.
    fn outcome(&self) -> Next {
        if !self.accumulator.finished() {
            return Next::End(End::provider(&unfinished_error()));
        }
        let stop_reason = self.accumulator.stop_reason();
        if stop_reason == Some(&StopReason::Refusal) {
            return Next::End(End::failed(ExchangeFailure::Refused, "the model refused to answer"));
        }
        if self.accumulator.tool_use_cut() {
            return Next::End(End::failed(ExchangeFailure::OutputTruncated, "the token limit cut through a tool call"));
        }
        if !self.accumulator.tool_uses().is_empty() {
            if stop_reason != Some(&StopReason::ToolUse) {
                warn!("Exchange {} asked for tools with stop reason {stop_reason:?}; running them", self.exchange_id);
            }
            return Next::ToolRound;
        }
        Next::End(match stop_reason {
            _ if !self.accumulator.has_text() => End::failed(ExchangeFailure::EmptyResponse, "the model ended its answer without text"),
            Some(StopReason::Other(reason)) => {
                warn!("Exchange {} ended with unknown stop reason {reason:?}; completing it", self.exchange_id);
                End::Completed
            }
            None => {
                warn!("Exchange {} ended with message_stop but without a stop reason; completing it", self.exchange_id);
                End::Completed
            }
            Some(_) => End::Completed,
        })
    }

    /// Answers the latest call's tool_use blocks, in order, with one tool Message, then publishes a
    /// tool block per call. The call is closed first and is then no longer the run's open call: a
    /// round that fails or finds the Exchange deleted leaves it as it was, since that request
    /// succeeded. A tool that runs longer than `TOOL_TIMEOUT` gets an error result. At the round
    /// limit the calls get an error result instead of running; asking for tools again after that
    /// ends the Exchange `ToolRoundLimit`. `messages` are this Exchange's: the call's assistant
    /// Message and the tool Message are added to them. Returns how the Exchange ends, if it does.
    async fn tool_round(
        &mut self,
        user_id: i32,
        messages: &mut Vec<messages::Model>,
        db: &DbConn,
        registry: &ExchangeRegistry,
        tmdb: &dyn Tmdb,
        clock: &dyn Clock,
    ) -> Result<Option<End>, SrvErr> {
        if self.refused {
            return Ok(Some(End::failed(ExchangeFailure::ToolRoundLimit, "the model asked for tools again after reaching the tool round limit")));
        }
        let message = self.message.clone().ok_or_else(|| SrvErr::Internal(format!("tool round of Exchange {} has no assistant message", self.exchange_id)))?;
        self.close_call(None, clock.now().fixed_offset(), db).await?;
        self.llm_call_id = None;
        messages.push(messages::Model { content: self.written.clone(), ..message });
        let user = match self.user.take() {
            Some(user) => user,
            None => CurrentUser::from(users::Entity::find_by_id(user_id).one(db).await?.ok_or(SrvErr::NotFound)?),
        };

        let refuse = self.rounds == config().assistant.max_tool_rounds;
        let tool_uses = self.accumulator.tool_uses();
        let mut outcomes = Vec::with_capacity(tool_uses.len());
        for tool_use in &tool_uses {
            outcomes.push(match &tool_use.input {
                input if refuse => failed(&tool_use.name, input.as_ref().unwrap_or(&Value::Null), ROUND_LIMIT_RESULT),
                None => failed(&tool_use.name, &Value::Null, "input is not a JSON object"),
                Some(input) => match timeout(TOOL_TIMEOUT, run_tool(&tool_use.name, input, &user, db, tmdb, clock)).await {
                    Ok(outcome) => outcome?,
                    Err(_) => failed(&tool_use.name, input, &format!("tool timed out after {} s", TOOL_TIMEOUT.as_secs())),
                },
            });
        }
        self.user = Some(user);
        if refuse {
            self.refused = true;
        } else {
            self.rounds += 1;
        }

        let content = Value::Array(tool_uses.iter().zip(&outcomes).map(|(tool_use, outcome)| result_block(&tool_use.id, outcome)).collect());
        let first_block = display_blocks(messages).len() as u32;
        let inserted = messages::ActiveModel {
            exchange_id: Set(self.exchange_id),
            role: Set(MessageRole::Tool),
            content: Set(content.clone()),
            llm_call_id: Set(None),
            created_at: Set(clock.now().fixed_offset()),
            ..Default::default()
        }
        .insert(db)
        .await;
        let inserted = match inserted {
            Ok(inserted) => inserted,
            Err(err) => {
                if !exchange_exists(self.exchange_id, db).await? {
                    return Ok(Some(End::Deleted));
                }
                return Err(err.into());
            }
        };
        messages.push(messages::Model { content, ..inserted });
        for (offset, outcome) in outcomes.into_iter().enumerate() {
            registry.publish(self.exchange_id, ExchangeEvent::ToolCall { block: first_block + offset as u32, call: outcome.display });
        }
        Ok(None)
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

/// The Conversation's Exchanges ordered by creation time then id, each with its Messages ordered by id.
async fn load_history(conversation_id: i32, db: &DbConn) -> Result<History, SrvErr> {
    let exchanges = exchanges::Entity::find()
        .filter(exchanges::Column::ConversationId.eq(conversation_id))
        .order_by_asc(exchanges::Column::CreatedAt)
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

/// The tool round budget an Exchange has used up, read from its stored `messages`: with `n` Tool
/// Messages, `n` rounds ran, up to `max`, and the round after the last allowed one was refused
/// (a refused round stores a Tool Message too, so a refused Exchange holds `max + 1`). Returns
/// `(rounds, refused)`. Deriving it from the database keeps a Retry from earning a fresh budget.
fn spent_rounds(messages: &[messages::Model], max: u32) -> (u32, bool) {
    let tool_messages = messages.iter().filter(|message| message.role == MessageRole::Tool).count() as u32;
    (tool_messages.min(max), tool_messages > max)
}

async fn exchange_exists(exchange_id: i32, db: &DbConn) -> Result<bool, SrvErr> {
    Ok(exchanges::Entity::find_by_id(exchange_id).one(db).await?.is_some())
}

/// Joins the deltas of each block, in the order the blocks first appear. A block has one kind,
/// so its number identifies it. Other events (`ToolCall`) pass through untouched.
fn merge(deltas: Vec<ExchangeEvent>) -> Vec<ExchangeEvent> {
    let mut merged: Vec<ExchangeEvent> = Vec::new();
    for delta in deltas {
        let Some((block, text)) = delta_parts(&delta) else {
            merged.push(delta);
            continue;
        };
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
pub(crate) fn idle_error() -> LlmError {
    LlmError { status: None, error_type: None, message: format!("no event for {} s", IDLE_TIMEOUT.as_secs()), response: None }
}

/// The error of a stream that ended without error before the model finished its message. Having
/// neither status nor error type, it classifies as `ProviderUnavailable`.
pub(crate) fn unfinished_error() -> LlmError {
    LlmError { status: None, error_type: None, message: "stream ended before the answer finished".into(), response: None }
}

/// A token count for an `i32` column; counts beyond its range are stored as `i32::MAX`.
pub(crate) fn tokens(count: Option<u64>) -> Option<i32> {
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
    use integrations::openrouter::{LlmEvent, LlmRole, LlmUsage, StopReason};
    use sea_orm::{DatabaseConnection, DbErr, Statement, Value};
    use serde_json::json;
    use views::conversations::{ExchangeEvent, ToolCallView};
    use super::*;
    use crate::test_support::{self, fixed_clock, MockLlm, MockTmdb};
    use crate::test_support::tmdb::UnansweringTmdb;
    use crate::test_support::db::{
        assert_no_statement_starting_with, assert_wrote, updated, with_final_answer, with_next_tool_round, with_resumed_exchange_run, with_started_exchange_run, with_tool_round,
    };
    use crate::test_support::fixtures::{conversation_model, exchange_model, llm_call_model, message_model, user_model};
    use crate::test_support::llm::{
        date_time_call, date_time_result, http_error, panicking_stream, stalled_stream, stream_error, text_answer, text_then_tool_use_answer, thinking_answer, thinking_then_text_answer,
        thinking_then_tool_use_answer, tool_use_answer, trickled_stream, with_answer, with_answers, with_error, with_recorded_answers, with_stream, UnansweringLlm,
    };

    /// Runs Exchange 7 with a subscriber attached from the start; returns whether the run
    /// returned `Ok`, the events the subscriber received, and the statements executed.
    async fn run(db: DatabaseConnection, llm: MockLlm) -> (bool, Vec<ExchangeEvent>, Vec<Statement>) {
        run_with_tmdb(db, llm, &MockTmdb::new()).await
    }

    /// Like `run`, with `tmdb` for the tools.
    async fn run_with_tmdb(db: DatabaseConnection, llm: MockLlm, tmdb: &dyn Tmdb) -> (bool, Vec<ExchangeEvent>, Vec<Statement>) {
        let registry = ExchangeRegistry::new();
        registry.open(7);
        let mut live = registry.subscribe(7).expect("open").live.expect("live");

        let result = run_exchange(7, &db, &llm, &registry, tmdb, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

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

    #[test]
    fn merge_passes_tool_call_through_when_deltas_are_joined() {
        test_support::init();
        let call = ExchangeEvent::ToolCall {
            block: 1,
            call: ToolCallView { name: "date_time".into(), query: None, media_type: None, count: None, titles: None, failed: false },
        };

        let merged = merge(vec![delta(0, "Hel"), call.clone(), delta(0, "lo")]);

        assert_eq!(merged, vec![delta(0, "Hello"), call]);
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

        let result = std::panic::AssertUnwindSafe(run_exchange(7, &db, &llm, &registry, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0)))
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

        let result = run_exchange(7, &db, &llm, &registry, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

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

        let result = run_exchange(7, &db, &UnansweringLlm, &registry, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

        assert!(result.is_ok());
        assert_eq!(live.recv().await, Some(failed("ProviderUnavailable", "no event for 60 s")));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from(json!({"status": null, "message": "no event for 60 s", "response": null}))]);
    }

    fn tool_call(block: u32, name: &str, failed: bool) -> ExchangeEvent {
        ExchangeEvent::ToolCall { block, call: ToolCallView { name: name.into(), query: None, media_type: None, count: None, titles: None, failed } }
    }

    fn role(role: MessageRole) -> Value {
        Value::from(sea_orm::ActiveEnum::to_value(&role))
    }

    #[tokio::test]
    async fn run_exchange_runs_tool_round_then_completes_when_model_answers_after_tool() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[date_time_call("tu_1")], StopReason::ToolUse),
            text_answer(&["It is noon"], StopReason::EndTurn),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![tool_call(0, "date_time", false), delta(1, "It is noon"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[role(MessageRole::Tool), Value::Int(None), Value::from(json!([date_time_result("tu_1")]))]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("tool_use"), Value::Json(None)]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_answers_all_calls_in_one_tool_message_when_round_has_two_calls() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[date_time_call("tu_1"), date_time_call("tu_2")], StopReason::ToolUse),
            text_answer(&["Done"], StopReason::EndTurn),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![
            tool_call(0, "date_time", false),
            tool_call(1, "date_time", false),
            delta(2, "Done"),
            ExchangeEvent::Completed { exchange_id: 7 },
        ]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 3);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[role(MessageRole::Tool), Value::from(json!([date_time_result("tu_1"), date_time_result("tu_2")]))]);
    }

    #[tokio::test]
    async fn run_exchange_numbers_tool_blocks_after_text_when_message_has_text_and_tool_use() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            text_then_tool_use_answer(&["Let me check"], &[date_time_call("tu_1")], StopReason::ToolUse),
            text_answer(&["It is noon"], StopReason::EndTurn),
        ]);

        let (ok, events, _) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![
            delta(0, "Let me check"),
            tool_call(1, "date_time", false),
            delta(2, "It is noon"),
            ExchangeEvent::Completed { exchange_id: 7 },
        ]);
    }

    #[tokio::test]
    async fn run_exchange_runs_tools_when_tool_use_arrives_with_end_turn() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[date_time_call("tu_1")], StopReason::EndTurn),
            text_answer(&["It is noon"], StopReason::EndTurn),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![tool_call(0, "date_time", false), delta(1, "It is noon"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([date_time_result("tu_1")]))]);
    }

    #[tokio::test]
    async fn run_exchange_completes_when_stop_reason_is_tool_use_without_tool_use_blocks() {
        test_support::init();

        let (ok, events, log) = run(answering_db(), with_answer(text_answer(&["Hi"], StopReason::ToolUse))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(0, "Hi"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_fails_with_output_truncated_when_max_tokens_cuts_tool_use() {
        test_support::init();
        let events = tool_use_answer(&[("tu_1", "mia_search", json!({"query": "type = movie"}))], StopReason::MaxTokens)
            .into_iter()
            .filter(|event| !matches!(event, Ok(LlmEvent::BlockStop { .. })))
            .collect();

        let (ok, events, log) = run(answering_db(), with_answer(events)).await;

        assert!(ok);
        assert_eq!(events, vec![failed("OutputTruncated", "the token limit cut through a tool call")]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 1);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
    }

    #[tokio::test]
    async fn run_exchange_refuses_tools_at_round_limit_then_fails_with_tool_round_limit() {
        test_support::init();
        let db = with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7);
        let db = with_next_tool_round(db, 15, 16, 17, 7);
        let db = with_final_answer(with_next_tool_round(db, 18, 19, 20, 7), 7, 21).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[date_time_call("tu_1")], StopReason::ToolUse),
            tool_use_answer(&[date_time_call("tu_2")], StopReason::ToolUse),
            tool_use_answer(&[date_time_call("tu_3")], StopReason::ToolUse),
            tool_use_answer(&[date_time_call("tu_4")], StopReason::ToolUse),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![
            tool_call(0, "date_time", false),
            tool_call(1, "date_time", false),
            tool_call(2, "date_time", true),
            failed("ToolRoundLimit", "the model asked for tools again after reaching the tool round limit"),
        ]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{
            "type": "tool_result",
            "tool_use_id": "tu_3",
            "content": "tool limit reached; answer with what you have",
            "is_error": true,
            "mia_display": {"name": "date_time", "failed": true},
        }]))]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 7);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
    }

    #[tokio::test]
    async fn run_exchange_completes_when_model_answers_after_refused_round() {
        test_support::init();
        let db = with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7);
        let db = with_next_tool_round(db, 15, 16, 17, 7);
        let db = with_final_answer(with_next_tool_round(db, 18, 19, 20, 7), 7, 21).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[date_time_call("tu_1")], StopReason::ToolUse),
            tool_use_answer(&[date_time_call("tu_2")], StopReason::ToolUse),
            tool_use_answer(&[date_time_call("tu_3")], StopReason::ToolUse),
            text_answer(&["It is noon"], StopReason::EndTurn),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![
            tool_call(0, "date_time", false),
            tool_call(1, "date_time", false),
            tool_call(2, "date_time", true),
            delta(3, "It is noon"),
            ExchangeEvent::Completed { exchange_id: 7 },
        ]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_fails_with_empty_response_when_final_call_has_no_text_after_tool_round() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            text_then_tool_use_answer(&["Let me check"], &[date_time_call("tu_1")], StopReason::ToolUse),
            thinking_answer("hmm", StopReason::EndTurn),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![
            delta(0, "Let me check"),
            tool_call(1, "date_time", false),
            thinking_delta(2, "hmm"),
            failed("EmptyResponse", "the model ended its answer without text"),
        ]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
    }

    #[tokio::test]
    async fn run_exchange_answers_with_error_result_when_tool_input_is_not_an_object() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[("tu_1", "mia_search", json!(["type = movie"]))], StopReason::ToolUse),
            text_answer(&["Sorry"], StopReason::EndTurn),
        ]);

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![tool_call(0, "mia_search", true), delta(1, "Sorry"), ExchangeEvent::Completed { exchange_id: 7 }]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{"type": "tool_use", "id": "tu_1", "name": "mia_search", "input": {}}]))]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{
            "type": "tool_result",
            "tool_use_id": "tu_1",
            "content": "input is not a JSON object",
            "is_error": true,
            "mia_display": {"name": "mia_search", "failed": true},
        }]))]);
    }

    #[tokio::test(start_paused = true)]
    async fn run_exchange_answers_with_failed_result_and_goes_on_when_tool_times_out() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let llm = with_answers(vec![
            tool_use_answer(&[("tu_1", "tmdb_search", json!({"query": "alien"}))], StopReason::ToolUse),
            text_answer(&["TMDB is slow"], StopReason::EndTurn),
        ]);

        let (ok, events, log) = run_with_tmdb(db, llm, &UnansweringTmdb).await;

        assert!(ok);
        assert_eq!(events, vec![
            ExchangeEvent::ToolCall {
                block: 0,
                call: ToolCallView { name: "tmdb_search".into(), query: Some("alien".into()), media_type: None, count: None, titles: None, failed: true },
            },
            delta(1, "TMDB is slow"),
            ExchangeEvent::Completed { exchange_id: 7 },
        ]);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(json!([{
            "type": "tool_result",
            "tool_use_id": "tu_1",
            "content": "tool timed out after 30 s",
            "is_error": true,
            "mia_display": {"name": "tmdb_search", "query": "alien", "failed": true},
        }]))]);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Completed)]);
    }

    #[tokio::test]
    async fn run_exchange_loads_conversation_owner_for_tools_when_tool_round_runs() {
        test_support::init();
        let exchange = exchange_model(7, 3, ExchangeStatus::InProgress);
        let db = test_support::db::mock()
            .append_query_results([[exchange.clone()]])
            .append_query_results([[conversation_model(3, 4)]])
            .append_query_results([[exchange]])
            .append_query_results([[message_model(1, 7, MessageRole::User, json!([{"type": "text", "text": "Hi"}]))]])
            .append_query_results([[entities::llm_calls::Model { user_id: Some(4), ..llm_call_model(11, 7) }]])
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1)])
            .append_query_results([[user_model(4)]])
            .append_query_results([[message_model(13, 7, MessageRole::Tool, json!([]))]])
            .append_query_results([[entities::llm_calls::Model { user_id: Some(4), ..llm_call_model(14, 7) }]]);
        let llm = with_answers(vec![
            tool_use_answer(&[date_time_call("tu_1")], StopReason::ToolUse),
            text_answer(&["It is noon"], StopReason::EndTurn),
        ]);

        let (ok, _, log) = run(with_final_answer(db, 7, 15).into_connection(), llm).await;

        assert!(ok);
        let users: Vec<&Statement> = log.iter().filter(|statement| statement.sql.contains(r#"FROM "users""#)).collect();
        assert_eq!(users.len(), 1, "expected one users lookup, got {users:?}");
        let values = &users[0].values.as_ref().expect("the users lookup is bound with values").0;
        assert!(values.contains(&Value::from(4)), "user id 4 not among {values:?}");
    }

    #[tokio::test]
    async fn run_exchange_stops_quietly_when_conversation_deleted_during_tool_round() {
        test_support::init();
        let db = with_started_exchange_run(test_support::db::mock(), 7)
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1)])
            .append_query_results([[user_model(1)]])
            .append_query_errors([DbErr::Custom("foreign key violation".into())])
            .append_query_results([Vec::<entities::exchanges::Model>::new()])
            .into_connection();
        let llm = with_answer(tool_use_answer(&[date_time_call("tu_1")], StopReason::ToolUse));

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, Vec::new());
        assert_eq!(statements_starting_with(&log, r#"UPDATE "llm_calls""#), 1);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("tool_use"), Value::Json(None)]);
        assert_no_statement_starting_with(&log, r#"UPDATE "exchanges""#);
    }

    #[tokio::test]
    async fn run_exchange_fails_with_internal_and_keeps_closed_call_when_tool_query_fails() {
        test_support::init();
        let db = with_started_exchange_run(test_support::db::mock(), 7)
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1)])
            .append_query_results([[user_model(1)]])
            .append_query_errors([DbErr::Custom("connection lost".into())])
            .append_exec_results([updated(1)])
            .into_connection();
        let llm = with_answer(tool_use_answer(&[("tu_1", "mia_search", json!({"query": "alien"}))], StopReason::ToolUse));

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert!(
            matches!(events.as_slice(), [ExchangeEvent::Failed { key, .. }] if key == "Internal"),
            "expected only an Internal failure, got {events:?}"
        );
        assert_eq!(statements_starting_with(&log, r#"UPDATE "llm_calls""#), 1);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[Value::from("tool_use"), Value::Json(None)]);
        assert_eq!(statements_starting_with(&log, r#"INSERT INTO "messages""#), 1);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status(ExchangeStatus::Failed)]);
    }

    #[tokio::test]
    async fn run_exchange_stops_quietly_before_calling_model_when_exchange_missing_from_history() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[exchange_model(7, 3, ExchangeStatus::InProgress)]])
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([Vec::<entities::exchanges::Model>::new()])
            .append_query_results([Vec::<entities::messages::Model>::new()])
            .into_connection();
        let mut llm = MockLlm::new();
        llm.expect_model().returning(|| "test/model".into());

        let (ok, events, log) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, Vec::new());
        test_support::db::assert_statement_count(&log, 4);
    }

    #[tokio::test]
    async fn run_exchange_sends_current_exchange_thinking_and_tool_blocks_in_next_request() {
        test_support::init();
        let db = with_final_answer(with_tool_round(with_started_exchange_run(test_support::db::mock(), 7), 12, 13, 14, 7), 7, 15).into_connection();
        let (llm, requests) = with_recorded_answers(vec![
            thinking_then_tool_use_answer("hmm", &[date_time_call("tu_1")], StopReason::ToolUse),
            text_answer(&["It is noon"], StopReason::EndTurn),
        ]);

        let (ok, _, _) = run(db, llm).await;

        assert!(ok);
        let requests = requests.lock().expect("requests lock");
        assert_eq!(requests.len(), 2);
        let next = &requests[1];
        assert_eq!(next.messages.len(), 3);
        assert_eq!(next.messages[1].role, LlmRole::Assistant);
        assert_eq!(next.messages[1].content, json!([
            {"type": "thinking", "thinking": "hmm", "signature": ""},
            {"type": "tool_use", "id": "tu_1", "name": "date_time", "input": {}},
        ]));
        assert_eq!(next.messages[2].role, LlmRole::User);
        assert_eq!(next.messages[2].content, json!([{
            "type": "tool_result",
            "tool_use_id": "tu_1",
            "content": r#"{"utc":"2024-01-01T12:00:00Z","timezone":"UTC"}"#,
            "cache_control": {"type": "ephemeral"},
        }]));
        assert_eq!(next.tools, crate::chat::tools::definitions());
    }

    fn tool_messages(count: i32) -> Vec<messages::Model> {
        (0..count).map(|n| message_model(n + 2, 7, MessageRole::Tool, json!([]))).collect()
    }

    #[test]
    fn spent_rounds_returns_zero_when_exchange_has_no_tool_message() {
        test_support::init();

        assert_eq!(spent_rounds(&[message_model(1, 7, MessageRole::User, json!([]))], 2), (0, false));
    }

    #[test]
    fn spent_rounds_counts_tool_messages_when_below_limit() {
        test_support::init();

        assert_eq!(spent_rounds(&tool_messages(1), 2), (1, false));
    }

    #[test]
    fn spent_rounds_is_not_refused_when_at_limit() {
        test_support::init();

        assert_eq!(spent_rounds(&tool_messages(2), 2), (2, false));
    }

    #[test]
    fn spent_rounds_is_refused_when_tool_messages_exceed_limit() {
        test_support::init();

        assert_eq!(spent_rounds(&tool_messages(3), 2), (2, true));
    }

    /// A user Message followed by `rounds` tool rounds (an assistant `tool_use` Message and its result).
    fn resumed_messages(rounds: i32) -> Vec<messages::Model> {
        let mut stored = vec![message_model(1, 7, MessageRole::User, json!([{"type": "text", "text": "Hi"}]))];
        for round in 0..rounds {
            let id = format!("tu_{round}");
            stored.push(message_model(2 + 2 * round, 7, MessageRole::Assistant, json!([{"type": "tool_use", "id": id, "name": "date_time", "input": {}}])));
            stored.push(message_model(3 + 2 * round, 7, MessageRole::Tool, json!([date_time_result(&id)])));
        }
        stored
    }

    #[tokio::test]
    async fn run_exchange_fails_with_tool_round_limit_when_stored_tool_messages_exceed_limit() {
        test_support::init();
        let db = with_resumed_exchange_run(test_support::db::mock(), 7, resumed_messages(3))
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1), updated(1)])
            .into_connection();
        let llm = with_answer(tool_use_answer(&[date_time_call("tu_9")], StopReason::ToolUse));

        let (ok, events, _) = run(db, llm).await;

        assert!(ok);
        assert_eq!(events, vec![failed("ToolRoundLimit", "the model asked for tools again after reaching the tool round limit")]);
    }

    #[tokio::test]
    async fn run_exchange_numbers_new_blocks_after_stored_blocks_when_exchange_resumed() {
        test_support::init();
        let mut stored = resumed_messages(1);
        stored[1] = message_model(2, 7, MessageRole::Assistant, json!([
            {"type": "text", "text": "Let me check"},
            {"type": "tool_use", "id": "tu_0", "name": "date_time", "input": {}},
        ]));
        let db = with_resumed_exchange_run(test_support::db::mock(), 7, stored)
            .append_query_results([[message_model(12, 7, MessageRole::Assistant, json!([]))]])
            .append_exec_results([updated(1), updated(1)])
            .into_connection();

        let (ok, events, _) = run(db, with_answer(text_answer(&["It is noon"], StopReason::EndTurn))).await;

        assert!(ok);
        assert_eq!(events, vec![delta(2, "It is noon"), ExchangeEvent::Completed { exchange_id: 7 }]);
    }

    #[tokio::test]
    async fn load_history_orders_exchanges_by_created_at_then_id() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([Vec::<entities::exchanges::Model>::new()])
            .append_query_results([Vec::<entities::messages::Model>::new()])
            .into_connection();

        load_history(3, &db).await.expect("history should load");

        let log = test_support::db::log(db);
        assert!(
            log[0].sql.contains(r#"ORDER BY "exchanges"."created_at" ASC, "exchanges"."id" ASC"#),
            "unexpected SQL: {}", log[0].sql
        );
    }

    #[tokio::test]
    async fn recover_interrupted_marks_in_progress_exchanges_failed() {
        test_support::init();
        let db = test_support::db::mock()
            .append_exec_results([updated(1), updated(2)])
            .into_connection();

        let marked = recover_interrupted(&db, &fixed_clock(2024, 1, 1, 12, 0, 0)).await.expect("recovered");

        assert_eq!(marked, 2);
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[
            status(ExchangeStatus::Failed),
            Value::from(json!({"key": "Interrupted", "debug_message": "server stopped while the exchange was running"})),
        ]);
        assert_wrote(&log, r#"UPDATE "llm_calls""#, &[
            Value::from(json!({"message": "interrupted"})),
            Value::from(sea_orm::ActiveEnum::to_value(&LlmCallPurpose::Title)),
        ]);
    }
}
