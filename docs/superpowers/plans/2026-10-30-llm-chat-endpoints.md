# LLM Chat Endpoints Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Plain LLM chat for Mia: create/list/read/delete Conversations, send a message, and stream the answer over SSE, with client state always equal to DB state.

**Architecture:** OpenRouter's Anthropic-compatible endpoint sits behind a new `Llm` trait in `integrations` (real `OpenRouterClient` over openrouter-rs, fake `MockLlm`). In `services::chat`, an Exchange runs as `Job::RunExchange`. It feeds provider events through a pure `Accumulator`, writes each coalesced chunk to Postgres, then publishes it through an in-process `ExchangeRegistry` that SSE responses subscribe to and can rejoin. Failures become typed `ExchangeFailure` keys.

**Tech Stack:** Rust 2024, Axum 0.8 (`axum::response::sse`), SeaORM 1.1 / Postgres, openrouter-rs 0.22, tokio, mockall, testcontainers.

**Spec:** `docs/superpowers/specs/2026-10-29-llm-chat-endpoints-design.md` (read it with this plan). Domain terms: `CONTEXT.md`. Test rules: `CLAUDE.md` § Testing.

## Global Constraints

- Layering: `api` → `services` → `integrations`/`entities`/`views`. All SeaORM queries in `services`. `services` never imports openrouter-rs types.
- Do not hand-roll an OpenRouter HTTP/SSE client. Use openrouter-rs `0.22` (`OpenRouterClient::stream_messages`).
- In `services/src`: no `Utc::now`, `tokio::spawn`, `std::fs`/`tokio::fs`. Timestamps come from `Clock`, and background work goes through `Jobs`.
- Parameter order: own params ending with `db`, then `llm, registry, tmdb, store, jobs, clock, random` (each only if needed).
- SSE has exactly three events: `text_delta {block, text}`, `exchange_completed {exchange_id}`, `exchange_failed {exchange_id, key, debug_message}`.
- `block` is Exchange-wide: the n-th text block with non-empty text across the Exchange's assistant Messages, from 0. The same rule drives the stream and `GET /conversations/{id}`.
- Write first, forward second. Nothing is published before its DB write commits. Terminal events go out only after the Exchange status commits.
- An assistant Message is created on the first non-empty content (text or thinking). No Message for a call that failed before any content. Never store an empty block.
- Every request sent to OpenRouter gets an `llm_calls` row, inserted before sending. Never delete from `llm_calls`.
- Failed Exchanges are left out of the history sent to the model.
- Error columns are JSONB. `exchanges.error` = `{"key","debug_message"}`. `llm_calls.error` = `{"status","message","response"}`.
- Failure keys: `ProviderRateLimited`, `ProviderUnavailable`, `ProviderOutOfCredits`, `ProviderAuthFailed`, `Refused`, `ProviderError`, `EmptyResponse`, `Interrupted`, `Internal`. Classify on HTTP status before streaming and on `error.error_type` during streaming. Never match message text.
- Config: `openrouter.api_key`, `openrouter.model`, `openrouter.max_tokens` (default `16384`). No thinking (`AnthropicThinking::Disabled`). Idle timeout is 60 s.
- Tests follow `CLAUDE.md`: `<function>_<expected outcome>_when_<condition>`, `test_support::init()` first, builders in `test_support` (never local), ignore reasons only `"requires docker"`, `"requires OpenRouter key"` (new), `"touches filesystem"`, `"requires TMDB token"`, `"known bug: …"`.
- After every task: `cargo test --workspace` and `cargo clippy --workspace --all-targets` with no new warnings.

## Review Focus

1. A client that disconnects mid-answer: the Exchange still completes and is fully stored (Task 8, `run_exchange_completes_when_subscriber_dropped`).
2. A burst of deltas already buffered: one DB write and one `text_delta` carrying the concatenated text, never ahead of the DB (Task 8, `run_exchange_coalesces_ready_deltas_into_one_write_and_one_event_when_stream_is_buffered`).
3. Rejoining an Exchange that finished while away: stored text is replayed, then the stored terminal event (Task 9, `subscribe_replays_from_db_with_terminal_event_when_exchange_finished`).
4. Two messages posted back-to-back in one Conversation: the second gets `ConversationExchangeInProgress`, even when both pass the pre-check and the partial unique index catches it (Task 9 unit test for the pre-check; Task 11 `is_exchange_in_progress_conflict_returns_true_when_partial_unique_index_violated` for the real Postgres error).
5. A model that only "thinks" and then stops: `EmptyResponse`, thinking stored, nothing displayed (Task 8, `run_exchange_fails_with_empty_response_when_only_thinking_arrived`).

---

### Task 1: JSONB error columns

**Files:**
- Modify: `migrations/src/m20261029_000001_llm_chat.rs` (`Exchanges::Error`, `LlmCalls::Error` → `.json_binary()`)
- Modify: `entities/src/entities/exchanges.rs`, `entities/src/entities/llm_calls.rs` (`error: Option<Json>`, `#[sea_orm(column_type = "JsonBinary", nullable)]`)
- Modify: `services/tests/postgres_migrations.rs` (add `Conversations, Exchanges, LlmCalls, Messages` to the existing `select_each!` in `entities_match_schema`; no new test)

**Interfaces:**
- Produces: `exchanges::Model.error: Option<Json>`, `llm_calls::Model.error: Option<Json>`.

- [ ] **Step 1: Implement.** Change both migration columns to `json_binary()` and both entity fields to `Option<Json>`. Edit the entities by hand to match generator output. Don't regenerate the whole entity directory.
- [ ] **Step 2: Verify.** `cargo build --workspace` should succeed, and `cargo test -p services --test postgres_migrations -- --ignored` should pass.
- [ ] **Step 3: Commit.** `git commit -m "Making chat error columns JSONB"`. Tell the user that a dev DB with the old migration applied needs `cargo run -- migrate down 1 && cargo run -- migrate up`.

---

### Task 2: `Llm` trait, types and `MockLlm`

**Files:**
- Create: `integrations/src/openrouter/mod.rs`, `integrations/src/openrouter/types.rs`
- Modify: `integrations/src/lib.rs` (`pub mod openrouter;`)
- Create: `services/src/test_support/llm.rs`. Modify: `services/src/test_support/mod.rs` (`pub mod llm; pub use llm::MockLlm;`)

**Interfaces:**
- Produces (in `integrations::openrouter`):

```rust
pub enum LlmRole { User, Assistant }
pub struct LlmMessage { pub role: LlmRole, pub content: serde_json::Value }   // array of Anthropic content blocks
pub struct LlmRequest { pub system: String, pub messages: Vec<LlmMessage> }
#[derive(Clone, Debug, PartialEq)] pub enum BlockKind { Text, Thinking, Other(String) }
#[derive(Clone, Debug, PartialEq)] pub enum StopReason { EndTurn, StopSequence, MaxTokens, ToolUse, Refusal, Other(String) }
impl StopReason { pub fn as_str(&self) -> &str }   // wire value, e.g. "end_turn"; for llm_calls.stop_reason
#[derive(Clone, Debug, Default, PartialEq)] pub struct LlmUsage { pub input_tokens: Option<u64>, pub output_tokens: Option<u64>, pub cache_read_tokens: Option<u64>, pub cache_creation_tokens: Option<u64> }
#[derive(Clone, Debug, PartialEq)] pub enum LlmEvent {
    MessageStart { request_id: Option<String>, usage: LlmUsage },
    BlockStart { index: u32, kind: BlockKind },
    TextDelta { index: u32, text: String },
    ThinkingDelta { index: u32, thinking: String },
    SignatureDelta { index: u32, signature: String },
    BlockStop { index: u32 },
    MessageDelta { stop_reason: Option<StopReason>, usage: LlmUsage },
    MessageStop,
}
#[derive(Clone, Debug, PartialEq)] pub struct LlmError {
    pub status: Option<u16>,          // HTTP status of a pre-stream error response
    pub error_type: Option<String>,   // OpenRouter canonical type; mid-stream errors only
    pub message: String,
    pub response: Option<serde_json::Value>, // raw provider JSON
}
pub type LlmEventStream = BoxStream<'static, Result<LlmEvent, LlmError>>;
#[async_trait] pub trait Llm: Send + Sync {
    fn model(&self) -> String;
    async fn stream_message(&self, request: LlmRequest) -> Result<LlmEventStream, LlmError>;
}
```

- Produces (in `services::test_support::llm`): `MockLlm` (`mockall::mock!`, like `MockTmdb`), plus:
  - `text_answer(chunks: &[&str], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>>`: MessageStart (request id `"gen-test"`, input 10), BlockStart 0 Text, one TextDelta per chunk, BlockStop 0, MessageDelta (stop, output 5), MessageStop.
  - `event_stream(events: Vec<Result<LlmEvent, LlmError>>) -> LlmEventStream` (`futures::stream::iter(..).boxed()`)
  - `http_error(status: u16, message: &str) -> LlmError`
  - `stream_error(error_type: &str, message: &str) -> LlmError` (`response` = `{"type": "<native>", "message", "error_type"}`)
  - `with_answer(events: Vec<Result<LlmEvent, LlmError>>) -> MockLlm` (expects one `stream_message`, `model()` returns `"test/model"`)

- [ ] **Step 1: Write the failing test** in `services/src/test_support/llm.rs`: `text_answer_streams_text_between_block_start_and_stop`. Collect `event_stream(text_answer(&["Hel", "lo"], StopReason::EndTurn))` and assert that items 2 and 3 are `TextDelta { index: 0, text: "Hel" }` and `{ .., "lo" }`, and that the last item is `MessageStop`.
- [ ] **Step 2: Run it and check that it fails.** `cargo test -p services test_support::llm` fails because the module is missing.
- [ ] **Step 3: Implement the types, the trait, the mock and the builders.** `integrations` already has `futures-util` and `async-trait`.
- [ ] **Step 4: Run it and check that it passes.** Same command.
- [ ] **Step 5: Commit.** `git commit -m "Adding Llm trait and MockLlm"`

---

### Task 3: `OpenRouterClient` and contract test

**Files:**
- Create: `integrations/src/openrouter/client.rs`. Modify: `integrations/Cargo.toml` (`openrouter-rs = "0.22"`), `integrations/src/openrouter/mod.rs`
- Create: `integrations/tests/openrouter_contract.rs`
- Modify: `CLAUDE.md` (Testing: add the command `cargo test -p integrations --test openrouter_contract -- --ignored  # OpenRouter contract test (live, free model; needs config.yaml)`, and add the ignore reason `"requires OpenRouter key"` to the list)

**Interfaces:**
- Consumes: Task 2 types.
- Produces: `OpenRouterClient::new(api_key: String, model: String, max_tokens: u32) -> Self`, `impl Llm for OpenRouterClient`. Also two private, unit-tested translators:
  - `fn translate_event(event: AnthropicMessagesStreamEvent) -> Option<Result<LlmEvent, LlmError>>`. `Ping` → `None`. `Error { error }` → `Err(LlmError { status: None, error_type: error["error_type"], message: error["message"], response: Some(error) })`. Content-block and message deltas are untyped `Value`: read `delta.type` ∈ `text_delta`/`thinking_delta`/`signature_delta`, and `stop_reason`/`usage` from `MessageDelta`.
  - `fn translate_error(error: OpenRouterError) -> LlmError`. `Api(ctx)` → `status: Some(ctx.status.as_u16())`, `message: ctx.message`, `response: ctx.metadata`. `HttpRequest`/`Io`/others → `status: None`.

- [ ] **Step 1: Write failing unit tests** in `client.rs`. Build openrouter-rs events with `serde_json::from_value`.
  - `translate_event_returns_text_delta_when_delta_is_text`
  - `translate_event_returns_error_with_error_type_when_stream_sends_error` (`error_type == Some("rate_limit_exceeded")`)
  - `translate_event_returns_stop_reason_and_output_tokens_when_message_delta`
  - `translate_event_skips_ping`
  - `translate_error_keeps_status_when_api_error` (status 402)
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p integrations openrouter`
- [ ] **Step 3: Implement.** Build the request with `AnthropicMessagesRequest::builder()`: `model`, `max_tokens`, `system`, the messages (our JSON content deserialised into `AnthropicMessageContent`), and thinking disabled. Call `client.stream_messages(&request)`. Map items through `translate_event`, dropping `None`. Find the exact builder setter names in `~/.cargo/registry/src/*/openrouter-rs-0.22.0/src/api/messages.rs`.
- [ ] **Step 4: Run them and check that they pass.** Same command.
- [ ] **Step 5: Write the contract test** `stream_message_yields_text_and_stop_reason_when_prompt_is_short`, marked `#[ignore = "requires OpenRouter key"]`. Read `openrouter.api_key` from `config.yaml` the way `tmdb_contract.rs` does, and always use model `openrouter/free` with `max_tokens` 512 (reasoning free models can spend 64 tokens thinking and produce no text). Assert that at least one non-empty `TextDelta` arrives, that a `MessageDelta` carries `Some(stop_reason)`, and that no `Err` arrives. Don't assert on model or text.
- [ ] **Step 6: Run it** with `cargo test -p integrations --test openrouter_contract -- --ignored`. It should pass. A 429 means the free tier is rate limited, so retry later and don't weaken the test.
- [ ] **Step 7: Commit.** `git commit -m "Adding OpenRouter client"`

---

### Task 4: Chat views and failure classification

**Files:**
- Create: `views/src/conversations/mod.rs`, `views/src/conversations/conversations.rs`. Modify: `views/src/lib.rs`
- Create: `services/src/services/chat/mod.rs`, `services/src/services/chat/failure.rs`. Modify: `services/src/services/mod.rs` (`pub mod chat;`)
- Modify: `services/Cargo.toml`: `serde_json` becomes a normal dependency (drop `dep:serde_json` from `test-support`), and `tokio` gets features `["time", "sync"]`. Dev `tokio` adds `"test-util"`.

**Interfaces:**
- Produces (in `views::conversations`; derive `Serialize` + `ToSchema`, plus `Deserialize` where noted; timestamps are `DateTime<Utc>`):

```rust
pub struct Conversation { pub id: i32, pub title: Option<String>, pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc> }
pub struct ConversationDetails { pub id: i32, pub title: Option<String>, pub created_at: DateTime<Utc>, pub updated_at: DateTime<Utc>, pub exchanges: Vec<ExchangeView> }
pub enum ExchangeStatusView { InProgress, Completed, Failed }            // serde snake_case
pub struct ExchangeView { pub id: i32, pub status: ExchangeStatusView, pub created_at: DateTime<Utc>, pub completed_at: Option<DateTime<Utc>>, pub content: String, pub blocks: Vec<DisplayBlock>, pub error: Option<ExchangeError> }
pub enum DisplayBlock { Text { text: String } }                          // #[serde(tag = "type", rename_all = "snake_case")]
#[derive(Deserialize)] pub struct ExchangeCreate { pub content: String }
#[derive(Deserialize, Clone, Debug, PartialEq)] pub struct ExchangeError { pub key: String, pub debug_message: String }
#[derive(Clone, Debug, PartialEq)] #[serde(untagged)]
pub enum ExchangeEvent { TextDelta { block: u32, text: String }, Completed { exchange_id: i32 }, Failed { exchange_id: i32, key: String, debug_message: String } }
impl ExchangeEvent { pub fn name(&self) -> &'static str }                 // "text_delta" | "exchange_completed" | "exchange_failed"
```

- Produces (in `services::chat::failure`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum ExchangeFailure { ProviderRateLimited, ProviderUnavailable, ProviderOutOfCredits, ProviderAuthFailed, Refused, ProviderError, EmptyResponse, Interrupted, Internal }
impl Display for ExchangeFailure   // the Debug name, as RuleViolation does
pub fn classify(error: &LlmError) -> ExchangeFailure
pub fn debug_message(error: &LlmError) -> String      // "<status>: <message>" | "<error_type>: <message>" | "<message>"
pub fn llm_call_error(error: &LlmError) -> serde_json::Value   // {"status", "message", "response"}
pub fn exchange_error(failure: ExchangeFailure, debug_message: String) -> ExchangeError
```

`classify` follows the spec table exactly. With `status: Some(_)`: 429 → RateLimited, 402 → OutOfCredits, 401/403 → AuthFailed, 408/5xx → Unavailable, otherwise ProviderError. With `status: None` and `error_type: Some(_)`: `rate_limit_exceeded`, then `provider_overloaded|provider_unavailable|server|timeout`, then `payment_required`, then `authentication|permission_denied`, then `refusal|content_policy_violation`, otherwise ProviderError. With neither (transport failure) → Unavailable.

- [ ] **Step 1: Write the failing tests.**
  - `classify_returns_rate_limited_when_status_429`
  - `classify_returns_provider_error_when_status_400`
  - `classify_returns_out_of_credits_when_status_402`
  - `classify_returns_auth_failed_when_status_401`
  - `classify_returns_unavailable_when_status_503`
  - `classify_returns_unavailable_when_no_status_and_no_error_type`
  - `classify_returns_refused_when_stream_error_is_content_policy_violation`
  - `classify_returns_provider_error_when_stream_error_type_unknown`
  - `debug_message_starts_with_status_when_status_present` (`"400: bad request"`)
  - `llm_call_error_keeps_raw_response_when_present` (`json!({"status": 400, "message": "bad request", "response": {...}})`)
  - In views: `exchange_event_serializes_without_tag_when_text_delta` (`{"block":0,"text":"Hi"}`, name `"text_delta"`)
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p services chat::failure` and `cargo test -p views`
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and check that they pass.** Same commands.
- [ ] **Step 5: Commit.** `git commit -m "Adding chat views and failure classification"`

---

### Task 5: Accumulator and display rule

**Files:**
- Create: `services/src/services/chat/accumulator.rs`, `services/src/services/chat/display.rs`

**Interfaces:**
- Consumes: `LlmEvent`, `StopReason`, `LlmUsage` (Task 2), `DisplayBlock` (Task 4).
- Produces:

```rust
pub struct Accumulator;
impl Accumulator {
    pub fn new(first_block: u32) -> Self;                       // Exchange-wide index of this call's first text block
    pub fn apply(&mut self, event: LlmEvent) -> Vec<(u32, String)>; // display deltas (block, text)
    pub fn content(&self) -> serde_json::Value;                 // Anthropic block array, empty blocks omitted
    pub fn has_content(&self) -> bool;                          // any non-empty text or thinking block
    pub fn has_text(&self) -> bool;
    pub fn stop_reason(&self) -> Option<&StopReason>;
    pub fn usage(&self) -> &LlmUsage;                           // MessageStart and MessageDelta merged
    pub fn request_id(&self) -> Option<&str>;
}
pub fn display_blocks(assistant_messages: &[messages::Model]) -> Vec<DisplayBlock>; // text blocks with non-empty text, message order then block order
pub fn user_text(message: &messages::Model) -> String;          // concatenated text blocks
```

A text block gets its Exchange-wide `block` number when its *first non-empty* delta arrives (`first_block` + number of text blocks numbered so far). That's the same rule `display_blocks` applies to stored content, so stream and reload agree. Stored shapes are `{"type":"text","text":…}` and `{"type":"thinking","thinking":…,"signature":…}`. Blocks of `BlockKind::Other` are ignored for now.

- [ ] **Step 1: Write the failing tests.**
  - `apply_returns_text_with_exchange_wide_block_when_text_delta` (`Accumulator::new(2)`, delta → `[(2, "Hi")]`)
  - `apply_numbers_second_text_block_next_when_provider_index_skips_thinking` (thinking at index 0, text at 1 → block 0)
  - `content_omits_empty_text_block_when_no_delta_arrived`
  - `content_holds_thinking_with_signature_when_thinking_streamed`
  - `has_content_returns_false_when_only_message_start_arrived`
  - `usage_merges_input_from_start_and_output_from_delta`
  - `display_blocks_matches_stream_numbering_when_content_reloaded`: feed the same events into an Accumulator, store its `content()` in a `messages::Model`, and assert that `display_blocks` gives a `Text` per streamed block with the concatenated text.
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p services chat::accumulator chat::display`
- [ ] **Step 3: Implement.** Add a `message_model(id, exchange_id, role, content)` builder to `test_support::fixtures`.
- [ ] **Step 4: Run them and check that they pass.**
- [ ] **Step 5: Commit.** `git commit -m "Adding chat stream accumulator"`

---

### Task 6: History and system prompt

**Files:**
- Create: `services/src/services/chat/history.rs`, `services/src/services/chat/system_prompt.md`

**Interfaces:**
- Produces:

```rust
pub const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");
pub fn build_request(current_exchange_id: i32, exchanges: &[(exchanges::Model, Vec<messages::Model>)]) -> LlmRequest;
```

The input is ordered by Exchange id, and each Exchange's messages by id. Keep `Completed` Exchanges and the current one, and skip all others. Role `user`/`tool` → `LlmRole::User`, and `assistant` → `LlmRole::Assistant`. Content is passed through verbatim, except that `"cache_control": {"type": "ephemeral"}` is added to the last block of the last message, on the request copy only. The system prompt is 2–4 sentences: the assistant lives in Mia, a personal movie and TV tracker. No date, nothing about the user.

- [ ] **Step 1: Write the failing tests.**
  - `build_request_skips_failed_exchanges`
  - `build_request_includes_current_exchange_when_in_progress`
  - `build_request_marks_last_block_for_caching`
  - `build_request_maps_tool_role_to_user`
  - `build_request_uses_system_prompt`
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p services chat::history`
- [ ] **Step 3: Implement.** Add `exchange_model(id, conversation_id, ExchangeStatus)` to `test_support::fixtures`.
- [ ] **Step 4: Run them and check that they pass.**
- [ ] **Step 5: Commit.** `git commit -m "Adding chat history building"`

---

### Task 7: Exchange registry

**Files:**
- Create: `services/src/services/chat/registry.rs`

**Interfaces:**
- Consumes: `ExchangeEvent` (Task 4).
- Produces:

```rust
pub struct ExchangeSubscription { pub replay: Vec<ExchangeEvent>, pub live: Option<tokio::sync::mpsc::UnboundedReceiver<ExchangeEvent>> }
#[derive(Default)] pub struct ExchangeRegistry;
impl ExchangeRegistry {
    pub fn new() -> Self;
    pub fn open(&self, exchange_id: i32);
    pub fn subscribe(&self, exchange_id: i32) -> Option<ExchangeSubscription>; // None when not open
    pub fn publish(&self, exchange_id: i32, event: ExchangeEvent);             // call only after the DB commit
    pub fn close(&self, exchange_id: i32);                                     // drops all senders
}
```

Each entry is a `std::sync::Mutex` over `{ blocks: Vec<String>, subscribers: Vec<UnboundedSender> }`. `publish` appends `TextDelta` text to `blocks[block]` and sends to every subscriber, dropping closed ones, all under the lock. `subscribe` takes `replay` (one `TextDelta` per block, in order) and registers its sender under the same lock, so there are no gaps or duplicates. Never hold the lock across an `.await`. The entry leaves room for a future cancel handle.

- [ ] **Step 1: Write the failing tests.**
  - `subscribe_returns_none_when_exchange_not_open`
  - `subscribe_replays_published_text_per_block_when_joining_late` (publish `"Hel"`, `"lo"` on block 0 and `"!"` on block 1 → replay `[TextDelta{0,"Hello"}, TextDelta{1,"!"}]`)
  - `publish_reaches_live_subscriber_after_replay_without_duplicates`
  - `publish_skips_dropped_subscriber`
  - `close_ends_live_stream`
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p services chat::registry`
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run them and check that they pass.**
- [ ] **Step 5: Commit.** `git commit -m "Adding exchange registry"`

---

### Task 8: Running an Exchange

**Files:**
- Create: `services/src/services/chat/exchange.rs`
- Modify: `services/src/jobs.rs` (variant, `JobDeps`, `run` dispatch), `services/src/test_support/jobs.rs` (`sizes` ignores other variants; add `job_deps(db: Arc<DbConn>, store: Arc<dyn ImageStore>) -> JobDeps` with `MockLlm::new()`, a new registry and a `fixed_clock(2024, 1, 1, 12, 0, 0)`), `services/tests/common/mod.rs` (use `job_deps`)
- Modify: `infrastructure/src/config.rs` (`pub openrouter: OpenRouterConfiguration { api_key: String, model: String, #[serde(default = "default_max_tokens")] max_tokens: u32 }`, default `16384`), `services/src/test_support/config.rs` (`api_key: "test-key", model: "test/model", max_tokens: 16384`)
- Modify: `api/src/lib.rs`. Build `Arc<dyn Llm>` from `OpenRouterClient::new(config().openrouter.api_key.clone(), config().openrouter.model.clone(), config().openrouter.max_tokens)` and one `Arc<ExchangeRegistry>`, and pass both, plus `clock`, into `JobDeps`. Keep both `Arc`s for Task 10's `AppState`.

**Interfaces:**
- Consumes: Tasks 2, 4–7.
- Produces:

```rust
// services::jobs
Job::RunExchange { exchange_id: i32 }
pub struct JobDeps { pub db: Arc<DbConn>, pub store: Arc<dyn ImageStore>, pub llm: Arc<dyn Llm>, pub registry: Arc<ExchangeRegistry>, pub clock: Arc<dyn Clock> }
// services::chat::exchange
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
pub async fn run_exchange(exchange_id: i32, db: &DbConn, llm: &dyn Llm, registry: &ExchangeRegistry, clock: &dyn Clock) -> Result<(), SrvErr>;
pub async fn recover_interrupted(db: &DbConn, clock: &dyn Clock) -> Result<u64, SrvErr>;  // number of Exchanges marked
```

`run_exchange` works like this:
1. Load the Exchange, its Conversation (for `user_id`), and all of the Conversation's Exchanges with their messages. The Accumulator starts at `display_blocks(<this Exchange's assistant messages>).len()`, which is 0 in plain chat.
2. `build_request`.
3. Insert the `llm_calls` row (purpose `exchange`, `llm.model()`, timestamps from `clock`).
4. `stream_message`.
5. Loop. Wait for `stream.next()` under `tokio::time::timeout(IDLE_TIMEOUT)`, `apply` the event, then drain whatever is already ready (`now_or_never`) and `apply` that too. If content changed, write it: insert the assistant Message on the first `has_content()`, otherwise update its `content`. Only then publish the merged deltas, one `TextDelta` per block per write.
6. End. Update `llm_calls` (stop reason, usage, `request_id`, `completed_at`, error) and the Exchange (status, `error`, `completed_at`) in one transaction. Then publish the terminal event and `close`.

Outcomes:
- `EndTurn`, `StopSequence`, `MaxTokens` and unknown `Other` with text → completed. Log a warning for `Other`.
- `Refusal` → `Refused`.
- A normal end without `has_text()` → `EmptyResponse`.
- `ToolUse` → `Internal`.
- Idle → `ProviderUnavailable`, with `llm_calls.error` message `"no event for 60 s"`.
- Any `Err` from our side → the Exchange fails `Internal` and the function still returns `Ok`.
- If a write finds the Exchange row gone (Conversation deleted), stop reading, record `{"message": "conversation deleted"}` on the `llm_calls` row, `close`, and return `Ok` without publishing.

`recover_interrupted` sets every `in_progress` Exchange to failed `Interrupted` (`exchange_error(Interrupted, "server stopped while the exchange was running")`), and gives every `llm_calls` row of those Exchanges with `completed_at IS NULL` the error `{"message": "interrupted"}` and `completed_at` = now.

- [ ] **Step 1: Write the failing tests.** Use the mock DB, `MockLlm` via `with_answer`, `FixedClock`, and a registry opened and subscribed before the run. Queue results in query order, and read events from the subscription's `live`.
  - `run_exchange_stores_answer_and_completes_when_stream_ends` (events `[TextDelta{0,"Hello"}, Completed{7}]`, `assert_wrote` messages content and exchanges status)
  - `run_exchange_coalesces_ready_deltas_into_one_write_and_one_event_when_stream_is_buffered` (`text_answer(&["Hel","lo"," world"])`, exactly one `INSERT INTO "messages"`, no `UPDATE "messages"`, events `[TextDelta{0,"Hello world"}, Completed{7}]`)
  - `run_exchange_completes_when_subscriber_dropped`
  - `run_exchange_keeps_partial_message_when_stream_fails_mid_answer` (`"Hel"`, then `stream_error("provider_overloaded", ..)` → message stored, `Failed{key: "ProviderUnavailable"}`)
  - `run_exchange_stores_no_message_when_request_fails_before_streaming` (`http_error(400, "bad")` → no message insert, exchange error `{"key":"ProviderError","debug_message":"400: bad"}`, llm call error `{"status":400,...}`)
  - `run_exchange_fails_with_empty_response_when_only_thinking_arrived` (thinking stored, no `TextDelta` published)
  - `run_exchange_fails_with_refused_when_stop_reason_is_refusal`
  - `run_exchange_completes_when_stop_reason_is_max_tokens`
  - `run_exchange_fails_with_unavailable_when_stream_goes_idle` (`#[tokio::test(start_paused = true)]`, stream = MessageStart then `futures::stream::pending()`)
  - `recover_interrupted_marks_in_progress_exchanges_failed`
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p services chat::exchange`
- [ ] **Step 3: Implement.** Add `Job::RunExchange` to `run` and `describe` (`"RunExchange (exchange {id})"`). Update every `JobDeps` construction.
- [ ] **Step 4: Run them and check that they pass**, then run `cargo test --workspace`.
- [ ] **Step 5: Commit.** `git commit -m "Running chat exchanges as jobs"`

---

### Task 9: Conversation service functions

**Files:**
- Create: `services/src/services/chat/conversations.rs`
- Modify: `services/src/infrastructure/rule_violation.rs` (`ConversationExchangeInProgress`, `ExchangeContentEmpty`), `services/src/test_support/fixtures.rs` (`conversation_model(id, user_id)`)

**Interfaces:**
- Consumes: Tasks 4–8.
- Produces:

```rust
pub async fn create(user: &CurrentUser, db: &DbConn, clock: &dyn Clock) -> Result<Conversation, SrvErr>;
pub async fn index(page: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<Conversation>, SrvErr>;  // updated_at desc
pub async fn details(conversation_id: i32, user: &CurrentUser, db: &DbConn) -> Result<ConversationDetails, SrvErr>;
pub async fn delete(conversation_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr>;
pub async fn start_exchange(conversation_id: i32, params: ExchangeCreate, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry, jobs: &dyn Jobs, clock: &dyn Clock) -> Result<ExchangeSubscription, SrvErr>;
pub async fn subscribe(conversation_id: i32, exchange_id: i32, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry) -> Result<ExchangeSubscription, SrvErr>;
```

- Another user's Conversation, or an Exchange outside the Conversation → `SrvErr::NotFound`.
- `start_exchange` works like this:
  1. Reject trimmed-empty content with `ExchangeContentEmpty`.
  2. Reject with `ConversationExchangeInProgress` if an `in_progress` Exchange exists. A `DbErr` for which `pub fn is_exchange_in_progress_conflict(err: &DbErr) -> bool` holds (a unique violation of `idx_exchanges_one_in_progress`) maps to the same violation.
  3. In one transaction: insert the Exchange (`in_progress`), insert the user Message `[{"type":"text","text": content}]` (content as sent, not trimmed), and set the Conversation's `updated_at`.
  4. `registry.open`, then `registry.subscribe`.
  5. Submit `Job::RunExchange`.
  6. Return the subscription.
- `subscribe` uses the registry when the Exchange is open. Otherwise it builds a replay from the DB (`display_blocks` → one `TextDelta` per block) plus the terminal event from status and `error`, with `live: None`. An Exchange still `in_progress` but not in the registry gets no terminal event; that only happens before startup recovery.
- `details` builds an `ExchangeView` per Exchange (`user_text`, `display_blocks`, `error` parsed from JSON).

- [ ] **Step 1: Write the failing tests.**
  - `create_sets_timestamps_from_clock`
  - `details_returns_not_found_when_conversation_of_other_user`
  - `details_returns_blocks_and_error_when_exchange_failed`
  - `index_orders_by_updated_at_desc`
  - `delete_returns_not_found_when_conversation_of_other_user`
  - `start_exchange_fails_with_content_empty_when_content_is_whitespace`
  - `start_exchange_fails_with_exchange_in_progress_when_one_is_running`
  - `start_exchange_submits_run_exchange_job_and_stores_user_message` (`RecordingJobs`, `assert_wrote` messages and the `updated_at` bump)
  - `subscribe_replays_from_db_with_terminal_event_when_exchange_finished`
- [ ] **Step 2: Run them and check that they fail.** `cargo test -p services chat::conversations`
- [ ] **Step 3: Implement.** Re-export from `services::chat::mod`: `conversations::*`, `exchange::{run_exchange, recover_interrupted}`, `registry::{ExchangeRegistry, ExchangeSubscription}`.
- [ ] **Step 4: Run them and check that they pass.**
- [ ] **Step 5: Commit.** `git commit -m "Adding conversation services"`

---

### Task 10: Endpoints and wiring

**Files:**
- Modify: `api/src/infrastructure/app_state.rs` (`llm: Arc<dyn Llm>`, `registry: Arc<ExchangeRegistry>`), `api/src/lib.rs` (put Task 8's `llm` and `registry` into `AppState`, and call `services::chat::recover_interrupted` before serving, logging an error on failure)
- Create: `api/src/endpoints/conversations.rs`. Modify: `api/src/endpoints/mod.rs`, `api/src/config/routes.rs` (`build()`), `api/src/openapi.rs`, `api/Cargo.toml` (`futures = "0.3"` if needed)
- Modify: `CLAUDE.md`. Add the config block's `openrouter:` section; change Architecture's `integrations/` line to "TMDB, OpenRouter"; add an `Llm` row `| integrations::openrouter::Llm | OpenRouterClient | MockLlm (mockall; builders in test_support::llm) |` and change "five traits" to "six"; add `llm, registry` to the parameter-order rule.

**Interfaces:**
- Consumes: Task 9 functions, Task 3 `OpenRouterClient`.
- Produces routes:

| Route | Handler | Success |
|---|---|---|
| `POST /conversations` | `conversations::create` | 201 `Conversation` |
| `GET /conversations` | `conversations::index` (`Query<PageReq>`) | 200 `[Conversation]` |
| `GET /conversations/{conversation_id}` | `conversations::details` | 200 `ConversationDetails` |
| `DELETE /conversations/{conversation_id}` | `conversations::delete` | 200 |
| `POST /conversations/{conversation_id}/exchanges` | `conversations::create_exchange` (`Json<ExchangeCreate>`) | 200 `text/event-stream` |
| `GET /conversations/{conversation_id}/exchanges/{exchange_id}/stream` | `conversations::stream` | 200 `text/event-stream` |

`fn sse(subscription: ExchangeSubscription) -> Sse<impl Stream<Item = Result<Event, Infallible>>>` chains `replay` with `live` and maps each event to `Event::default().event(e.name()).json_data(&e)`, using `KeepAlive::default()`. Service errors before the stream go through `IntoApiResponse` as JSON.

- [ ] **Step 1: Implement the `AppState` fields and startup recovery, then build.** `cargo build --workspace` must succeed.
- [ ] **Step 2: Implement handlers, routes and OpenAPI paths.** Document the SSE responses with `content_type = "text/event-stream"` and body `ExchangeEvent`.
- [ ] **Step 3: Verify by hand.** Add an `openrouter` section to your local `config.yaml` (model `openrouter/free`), then run `cargo run -- serve`. Log in, `POST /conversations`, then `curl -N -X POST .../conversations/{id}/exchanges -H 'Authorization: Bearer …' -d '{"content":"Hi"}'`. You should see `event: text_delta` lines followed by `event: exchange_completed`. `GET /conversations/{id}` then shows the same text in `blocks[0]`. `GET .../exchanges/{eid}/stream` replays it and ends with `exchange_completed`.
- [ ] **Step 4: Run** `cargo test --workspace` and `cargo clippy --workspace --all-targets`.
- [ ] **Step 5: Commit.** `git commit -m "Adding conversation endpoints"`

---

### Task 11: Postgres tests for chat

Only what the mock DB cannot prove: the real Postgres error from the partial unique index, the bulk recovery SQL, behaviour when rows vanish under a running Exchange, and one end-to-end run linking it all together. Everything else stays in unit tests.

**Files:**
- Create: `services/tests/postgres_chat.rs`. Modify: `services/tests/common/seed.rs` (`conversation(db, &user) -> conversations::Model`, `exchange(db, conversation_id, ExchangeStatus) -> exchanges::Model`, `llm_call(db, &user, exchange_id) -> llm_calls::Model` with `completed_at` null)

**Interfaces:**
- Consumes: Task 9 functions including `is_exchange_in_progress_conflict`, `job_deps` (Task 8). Use `InlineJobs` built from `JobDeps` with a `MockLlm` that has expectations set, and a shared registry.

- [ ] **Step 1: Write the tests**, all `#[ignore = "requires docker"]`.
  - `is_exchange_in_progress_conflict_returns_true_when_partial_unique_index_violated`: seed an `in_progress` Exchange, insert a second `in_progress` one for the same Conversation directly, and assert the function holds for the returned `DbErr`.
  - `recover_interrupted_marks_exchange_failed_and_closes_open_llm_call`: an `in_progress` and a `completed` Exchange. Only the first changes, and its open `llm_calls` row gets `completed_at` and an error.
  - `run_exchange_stops_quietly_when_conversation_deleted_mid_exchange`. The `MockLlm` returns a `futures::stream::unfold` stream that deletes the Conversation through a cloned `Arc<DbConn>` after its first delta. Assert that `run_exchange` returns `Ok`, and that the `llm_calls` row survives with `exchange_id` null and error `{"message": "conversation deleted"}`.
  - `start_exchange_persists_answer_and_ledger_when_stream_completes`: one end-to-end run. There are user and assistant messages, the `llm_calls` row has usage and `completed_at`, the Exchange is `completed`, and `details` shows the text in `blocks[0]`.
- [ ] **Step 2: Run them.** `cargo test -p services --test postgres_chat -- --ignored` should pass. If a test exposes a bug in Tasks 8–9, fix the code; these are new code, not characterization tests.
- [ ] **Step 3: Run the full suite.** `cargo test -p services --no-fail-fast --tests -- --ignored` should pass, apart from the known-bug tests that already fail by design.
- [ ] **Step 4: Commit.** `git commit -m "Adding chat Postgres tests"`
