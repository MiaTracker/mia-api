# Conversation Titles Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `POST /conversations` takes the first message and streams; a cheap second model titles the Conversation in parallel; the title and a final `stream_end` reach the client on the creating stream.

**Architecture:** Three new `ExchangeEvent` variants. The Exchange's registry entry is untouched. The title gets its own `oneshot` channel inside `ExchangeRegistry`, keyed by Conversation id, filled by a new `Job::GenerateTitle`. `ExchangeSubscription::into_events()` assembles every SSE response: replay, live events, the held-back title, then `stream_end`.

**Tech Stack:** Rust workspace (Axum, SeaORM, tokio, mockall, `integrations::openrouter`). Domain terms from `CONTEXT.md`.

**Spec:** `docs/superpowers/specs/2026-11-05-conversation-titles-design.md`

**Branch:** work on a new branch `conversation-titles` (master is the default branch).

## Global Constraints

- The title model differs from the main one: `openrouter.title_model` (optional) must not equal `openrouter.model`; absent means titling is off.
- The title client reuses `api_key` and `data_policy`, has no reasoning, and uses `max_tokens` 64 (plan's call, constant `TITLE_MAX_TOKENS`).
- The title call has no timeout of its own beyond the 60 s stream idle timeout (`exchange::IDLE_TIMEOUT`).
- The title write does not touch `conversations.updated_at`.
- The title's `llm_calls` row has purpose `title`, `user_id` and `conversation_id` set, `exchange_id` null.
- The Exchange's run, registry entry, `is_open` check and Retry are not changed by the title in any way.
- Only the creating stream carries `conversation_created` and `title`; every SSE stream the server ends ends with `stream_end`.
- `stream_end` is sent whatever the reason the sources ended; it never means "the run succeeded".
- Write to the database before publishing the matching event. Fixed query count, no per-item loops.
- No migration. No direct `tokio::spawn`, `Utc::now`, `Uuid::new_v4` in `services/src`; use `Jobs`, `Clock`, `Random`.
- Services parameter order: own parameters ending with `db`, then `llm, registry, tmdb, store, jobs, clock, random`; a function takes only what it or a callee needs.
- Test names `<function>_<expected outcome>_when_<condition>`; `test_support::init()` first in every test; builders go in `test_support::fixtures`, never locally; Postgres tests `#[ignore = "requires docker"]`.
- Run `cargo test --workspace` and `cargo clippy --workspace --all-targets` (no new warnings) after each task.

## Review Focus

Failure modes the spec implies; each has a test in the owning task.

1. The title finishes before the Exchange does: it must still be sent after the terminal event (Task 3).
2. The title job panics: the creating response must still end, with `stream_end` (Task 4).
3. The first Exchange fails and is retried while its title is pending: Retry must succeed at once and carry no `title` (Task 5).
4. The client disconnects before the title arrives: the job still writes the title and publishing to a dropped receiver is harmless, leaving no entry behind (Tasks 3 and 4).
5. A title with quotes, newlines, runs of spaces or multi-byte characters beyond the length cap: cleaned without panicking at a char boundary (Task 4).
6. The Conversation is deleted before the title job starts or while it runs: no model call or no publish, never an error (Task 4).

---

### Task 1: New stream events in the views

**Files:**
- Modify: `views/src/conversations/conversations.rs` (`ExchangeEvent`, `name()`, tests)
- Modify: `services/src/services/chat/registry.rs:~80-92` (`publish` match)

**Interfaces:**
- Produces: `ExchangeEvent::ConversationCreated { conversation_id: i32, exchange_id: i32 }` (name `conversation_created`), `ExchangeEvent::Title { title: String }` (name `title`), `ExchangeEvent::StreamEnd {}` (name `stream_end`). The enum stays `#[serde(untagged)]`.

- [ ] **Step 1: Write the failing tests** in `views/src/conversations/conversations.rs` `mod tests`:

```rust
#[test]
fn exchange_event_serializes_both_ids_when_conversation_created() {
    let event = ExchangeEvent::ConversationCreated { conversation_id: 4, exchange_id: 9 };
    assert_eq!(serde_json::to_value(&event).unwrap(), json!({"conversation_id": 4, "exchange_id": 9}));
    assert_eq!(event.name(), "conversation_created");
}

#[test]
fn exchange_event_serializes_title_when_title() {
    let event = ExchangeEvent::Title { title: "Alien rewatch".into() };
    assert_eq!(serde_json::to_value(&event).unwrap(), json!({"title": "Alien rewatch"}));
    assert_eq!(event.name(), "title");
}

#[test]
fn exchange_event_serializes_empty_object_when_stream_end() {
    let event = ExchangeEvent::StreamEnd {};
    assert_eq!(serde_json::to_value(&event).unwrap(), json!({}));
    assert_eq!(event.name(), "stream_end");
}
```

- [ ] **Step 2: Run** `cargo test -p views` — Expected: FAIL to compile (variants undefined).
- [ ] **Step 3: Add the three variants and their `name()` arms.** In `ExchangeRegistry::publish` add an arm `ExchangeEvent::ConversationCreated { .. } | ExchangeEvent::Title { .. } | ExchangeEvent::StreamEnd {} => return,` with a comment that these are never published to an Exchange entry (they belong to the response, see `into_events`).
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS (utoipa `ToSchema` must still derive; if it rejects the empty struct variant, give `StreamEnd` a `#[schema(...)]`-free alternative such as a `StreamEnd {}` with `#[serde(...)]` unchanged and adjust only the derive attributes, keeping the JSON `{}`).
- [ ] **Step 5: Commit** `feat: add conversation_created, title and stream_end events` (with the Co-Authored-By trailer).

---

### Task 2: `openrouter.title_model` configuration and client

**Files:**
- Modify: `infrastructure/src/config.rs` (`OpenRouterConfiguration`, new `validate`, `load_config`, tests)
- Modify: `services/src/test_support/config.rs:13` (add `title_model: Some("test/title-model".into())`)
- Modify: `services/src/infrastructure/llm.rs` (new `openrouter_title_llm`, `TITLE_MAX_TOKENS`, tests)
- Modify: `CLAUDE.md` config reference (add `title_model` under `openrouter`, with comment "optional; titles conversations with a cheaper model, must differ from model; absent disables titling")

**Interfaces:**
- Produces: `OpenRouterConfiguration.title_model: Option<String>` (`#[serde(default)]`); `OpenRouterConfiguration::validate(&self) -> Result<(), String>`; `pub fn openrouter_title_llm(config: &OpenRouterConfiguration) -> Option<Arc<dyn Llm>>` re-exported next to `openrouter_llm`; `pub const TITLE_MAX_TOKENS: u32 = 64`.

- [ ] **Step 1: Write the failing tests.** In `config.rs`: `openrouter_configuration_has_no_title_model_when_absent` (parse `api_key: k\nmodel: m\n`, assert `None`); `openrouter_configuration_reads_title_model_when_present`; `validate_fails_when_title_model_equals_model` expecting `Err("openrouter.title_model must differ from openrouter.model".into())`; `validate_succeeds_when_title_model_absent_or_different`. In `llm.rs`: `openrouter_title_llm_returns_none_when_title_model_absent`; `openrouter_title_llm_uses_title_model_when_present` (assert `.model() == "t/m"` on a config with `model: m`, `title_model: t/m`).
- [ ] **Step 2: Run** `cargo test -p infrastructure -p services openrouter` — Expected: FAIL (compile).
- [ ] **Step 3: Implement.** `validate` is called in `load_config` next to `assistant.validate()` with the same `fail!` style. `openrouter_title_llm` builds `OpenRouterClient::new(api_key, title_model, TITLE_MAX_TOKENS, None, data_policy(..))`.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat: add openrouter.title_model configuration`.

---

### Task 3: Title channel and `ExchangeSubscription::into_events`

**Files:**
- Modify: `services/src/services/chat/registry.rs` (title channel, `ExchangeSubscription.title`, `into_events`, tests)
- Modify: `services/src/services/chat/conversations.rs:163` and `api/src/endpoints/conversations.rs:161` (add `title: None` / `..` so they compile; Task 6 rewrites the latter)

**Interfaces:**
- Consumes: Task 1 variants.
- Produces:
  - `ExchangeRegistry::open_title(&self, conversation_id: i32) -> tokio::sync::oneshot::Receiver<String>`: registers a sender, replacing any earlier one.
  - `ExchangeRegistry::publish_title(&self, conversation_id: i32, title: String)`: removes the sender and sends; no-op when none or when the receiver is gone.
  - `ExchangeRegistry::close_title(&self, conversation_id: i32)`: removes the sender (the receiver then resolves as closed); no-op when none.
  - `ExchangeSubscription { replay: Vec<ExchangeEvent>, live: Option<UnboundedReceiver<ExchangeEvent>>, title: Option<oneshot::Receiver<String>> }`.
  - `ExchangeSubscription::into_events(self) -> impl futures::Stream<Item = ExchangeEvent> + Send`: yields `replay`, then `live` until its channel closes, then — only if `title` is `Some` — `ExchangeEvent::Title { title }` if the receiver resolves with a value (nothing if it closed), then `ExchangeEvent::StreamEnd {}`, then ends.

- [ ] **Step 1: Write the failing tests** (`#[tokio::test]` where async; collect the stream with `StreamExt::collect`):
  - `open_title_delivers_title_when_published_before_receiver_polled`: open, `publish_title(5, "T")`, `rx.await == Ok("T")`.
  - `publish_title_does_nothing_when_receiver_dropped`: open, drop receiver, publish, then publishing again is also a no-op (entry gone; assert second `publish_title` and `close_title` do not panic).
  - `close_title_resolves_receiver_as_closed_when_closed_without_publishing`: `rx.await.is_err()`.
  - `into_events_ends_with_stream_end_when_subscription_has_only_replay`.
  - `into_events_yields_replay_live_then_stream_end_when_live_closes`.
  - `into_events_yields_title_after_terminal_event_when_title_published_first`: publish the title, then send `Completed`, drop the sender; expect `[.., Completed, Title, StreamEnd]`.
  - `into_events_waits_for_title_when_exchange_ends_first`: drive with `tokio::join!` — close live, then publish the title after a `tokio::task::yield_now()`; expect `[Completed, Title, StreamEnd]`.
  - `into_events_skips_title_when_title_channel_closed_without_publishing`: expect `[Completed, StreamEnd]`.
- [ ] **Step 2: Run** `cargo test -p services registry` — Expected: FAIL (compile).
- [ ] **Step 3: Implement.** Add `titles: Mutex<HashMap<i32, oneshot::Sender<String>>>` to `ExchangeRegistry` (same `lock` helper); build `into_events` with `futures::stream::iter(replay).chain(..)` and an async-block tail for title and end, so the title is awaited only after `live` ends.
- [ ] **Step 4: Run** `cargo test --workspace` — Expected: PASS.
- [ ] **Step 5: Commit** `feat: add title channel and the combined event stream`.

---

### Task 4: `Job::GenerateTitle` and `generate_title`

**Files:**
- Create: `services/src/services/chat/title.rs` (`generate_title`, `clean_title`, `TITLE_PROMPT`, tests); add `pub mod title;` to `chat/mod.rs`
- Create: `services/src/services/chat/title_prompt.md`
- Modify: `services/src/jobs.rs` (`Job::GenerateTitle`, `describe`, `JobDeps.title_llm`, `run`, tests)
- Modify: `services/src/services/chat/exchange.rs` (make `tokens`, `idle_error`, `unfinished_error` `pub(crate)`)
- Modify: `services/src/test_support/jobs.rs` (`job_deps` gets `title_llm: None`; `sizes` match arm for `GenerateTitle`), `services/src/services/refresh.rs:28` (`title_llm: None`), `services/tests/postgres_chat.rs` (both `JobDeps` literals: `title_llm: None`), `api/src/lib.rs:48` (`title_llm: openrouter_title_llm(&config().openrouter)`)

**Interfaces:**
- Consumes: Task 2 `openrouter_title_llm`; Task 3 `open_title` / `publish_title` / `close_title`.
- Produces:
  - `Job::GenerateTitle { conversation_id: i32 }`, `describe()` = `"GenerateTitle (conversation {id})"`.
  - `JobDeps.title_llm: Option<Arc<dyn Llm>>`.
  - `pub async fn generate_title(conversation_id: i32, db: &DbConn, title_llm: &dyn Llm, registry: &ExchangeRegistry, clock: &dyn Clock)` — returns `()`; logs failures; never errors.
  - `pub fn clean_title(raw: &str) -> Option<String>`.
  - `jobs::run` arm: `GenerateTitle` calls `generate_title` when `title_llm` is `Some`, otherwise `registry.close_title(conversation_id)`; returns `Ok(())` either way.

Behaviour of `generate_title` (decisions the signature does not give):
- A guard closes the title channel (`close_title`) when the function ends or panics, after any publish (publish removes the entry first, so the guard is then a no-op).
- Reads the Conversation (`conversations::find_by_id`) and its first user Message (one query: the lowest-id `role = user` message over the Conversation's Exchanges). If either is missing, return before calling the model, writing no ledger row.
- Request: `system` = `TITLE_PROMPT` (`include_str!("title_prompt.md")`), one `LlmRole::User` message with content `[{"type":"text","text": "The first message of the conversation:\n\n{text}"}]` where `{text}` is `display::user_text(&message)`, `tools` empty.
- Inserts the `llm_calls` row first (purpose `Title`, `user_id`, `conversation_id`, `exchange_id` None, `model` = `title_llm.model()`, `created_at` = clock). Reads the stream with `Accumulator::new(0)` under `IDLE_TIMEOUT` exactly as `Run::call`/`read` do, then closes the row once (stop reason, tokens, request id, `error`), reusing `llm_call_error`.
- Failure (pre-stream error, mid-stream error, idle timeout, stream not `finished()`, stop reason `Refusal`, `clean_title` returns `None`): close the row with the error, log with `log::warn!`, publish nothing.
- Success: close the row, then `UPDATE conversations SET title = ? WHERE id = ?` (no `updated_at`); only when it affected a row, `registry.publish_title(conversation_id, title)`.
- `clean_title`: collapse every run of whitespace to one space, trim, strip one layer of matching surrounding quotes (`"…"`, `'…'`, `“…”`, `‘…’`, backticks), re-trim, cut to at most 80 `char`s (no ellipsis), `None` when empty.
- `title_prompt.md` content (exact): `Write a short title, at most six words, for a conversation that begins with the message the user gives you. Write it in the language of the message. Answer with the title only: no quotes, no full stop at the end, no explanation.`

- [ ] **Step 1: Write the failing tests.**
  - `clean_title` (pure): `clean_title_collapses_whitespace_when_title_has_newlines_and_runs_of_spaces` (`"  Alien \n  rewatch "` → `Some("Alien rewatch")`); `clean_title_strips_quotes_when_title_is_quoted` (`"\"Alien\""` and `"“Alien”"` → `Some("Alien")`); `clean_title_returns_none_when_title_is_blank_or_only_quotes` (`"  "`, `"\"\""`); `clean_title_cuts_at_char_boundary_when_title_longer_than_80_chars` (100 × `é` → 80 `é`, no panic).
  - `generate_title` (mock DB + `MockLlm` + `text_answer`, queue order: conversation row, first user message row, inserted `llm_calls` row with `exchange_id: None`, then execs `updated(1)` for closing the call and `updated(1)` for the title update): `generate_title_publishes_title_when_model_answers` (open the title channel first; assert the receiver gets the cleaned title, `assert_wrote(&log, r#"UPDATE "conversations""#, &[Value::from("Alien rewatch")])` and that the `llm_calls` INSERT carries purpose `title`); `generate_title_does_not_touch_updated_at_when_writing_title` (assert the written values of the conversations UPDATE contain the title only — use `assert_wrote` with exactly `[title, id]`); `generate_title_publishes_nothing_when_provider_rejects_request` (expect the call row closed with the error, no `UPDATE "conversations"` via `assert_no_statement_starting_with`, receiver resolves closed); `generate_title_publishes_nothing_when_stream_fails_midway`; `generate_title_publishes_nothing_when_answer_has_no_text`; `generate_title_publishes_nothing_when_stream_is_idle_for_60_s` (same technique as the existing idle-timeout test in `exchange.rs`; grep `no event for 60 s`); `generate_title_publishes_nothing_when_conversation_deleted_before_title_written` (title UPDATE returns `updated(0)`; call row still closed); `generate_title_calls_no_model_when_conversation_is_gone` (empty first query, `MockLlm` with no expectations, `assert_statement_count(&log, 1)`); `generate_title_closes_title_channel_when_model_call_panics` (mock `stream_message` panics; run under `AssertUnwindSafe(..).catch_unwind()`; assert the panic propagated and the receiver resolved closed); `generate_title_leaves_no_entry_behind_when_receiver_dropped` (drop the receiver before running; assert no panic and a later `publish_title` is a no-op).
  - `jobs`: `describe_names_conversation_when_generate_title` (`format!("{:?}", Job::GenerateTitle { conversation_id: 4 }) == "GenerateTitle (conversation 4)"`); `run_closes_title_channel_when_no_title_model_configured` (`job_deps` has `title_llm: None`; open the title channel in its registry, run the job, receiver resolves closed, `run` returned `Ok`).
- [ ] **Step 2: Run** `cargo test -p services title jobs` — Expected: FAIL (compile).
- [ ] **Step 3: Implement** per the behaviour list above, and fix the `JobDeps` literals listed under Files.
- [ ] **Step 4: Run** `cargo test --workspace` and `cargo clippy --workspace --all-targets` — Expected: PASS, no new warnings.
- [ ] **Step 5: Commit** `feat: generate conversation titles with a second model`.

---

### Task 5: Create a Conversation from its first message

**Files:**
- Modify: `services/src/services/chat/conversations.rs` (`create`, shared insert helper, tests)
- Modify: `services/tests/postgres_chat.rs`, `services/tests/postgres_transactions.rs` (new ignored tests)

**Interfaces:**
- Consumes: Tasks 3 and 4 (`open_title`, `Job::GenerateTitle`, `ExchangeSubscription.title`).
- Produces: `pub async fn create(params: ExchangeCreate, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry, jobs: &dyn Jobs, clock: &dyn Clock) -> Result<ExchangeSubscription, SrvErr>` replacing the old `create(user, db, clock)`. Its subscription's `replay` is exactly `[ExchangeEvent::ConversationCreated { conversation_id, exchange_id }]`, `live` is the Exchange's live channel, `title` is `Some` iff `config().openrouter.title_model.is_some()`.

Behaviour: reject blank content with `ExchangeContentEmpty` before any statement; one transaction inserts the Conversation (`title` null, `created_at` = `updated_at` = clock), the first Exchange (`in_progress`, `created_at` = clock) and the user Message — reuse the existing exchange/message insert code by extracting it into a helper that takes the transaction rather than duplicating it, and keep `start_exchange` using the same helper; then `registry.open(exchange_id)`, subscribe, `open_title(conversation_id)` when titling is on, submit `Job::RunExchange { exchange_id }` and then, when titling is on, `Job::GenerateTitle { conversation_id }`. The test config has `title_model` set, so unit tests run with titling on; the off branch is the one-line `config()` condition and is covered only through Task 4's `run` test.

- [ ] **Step 1: Write the failing tests** (replace `create_sets_timestamps_from_clock`):
  - `create_fails_with_content_empty_when_content_is_whitespace`: `assert_single_violation`, `assert_statement_count(.., 0)`, no jobs.
  - `create_returns_ids_and_submits_both_jobs_when_content_given`: mock queue conversation insert (`conversation_model(3,1)`), exchange insert (`exchange_model(9,3,InProgress)`), message insert; assert `replay == vec![ExchangeEvent::ConversationCreated { conversation_id: 3, exchange_id: 9 }]`, `live.is_some()`, `title.is_some()`, `jobs.take()` matches `[Job::RunExchange { exchange_id: 9 }, Job::GenerateTitle { conversation_id: 3 }]`, and the Message insert carries the trimmed-as-given content (same assertion style as `start_exchange_submits_run_exchange_job_and_stores_user_message`).
  - `create_sets_timestamps_from_clock`: conversation INSERT carries `[1, clock, clock]` values via `assert_wrote`.
  - `retry_exchange_succeeds_when_title_channel_open`: copy the existing successful-retry test, call `registry.open_title(3)` first, assert it still returns a subscription whose `replay`/`title` contain no title and that `jobs.take()` is only `[Job::RunExchange { exchange_id: 7 }]`.
  - Postgres (`#[ignore = "requires docker"]`): `create_persists_conversation_exchange_and_message_when_content_given` (in `postgres_chat.rs`, `InlineJobs` with a `MockLlm` answering the Exchange and a title `MockLlm` answering `"Alien rewatch"`, assert `conversations.title == Some("Alien rewatch")`, an `llm_calls` row of purpose `Title` with `exchange_id` null, and that `subscription.into_events()` yields `ConversationCreated`, the blocks, `Completed`, `Title`, `StreamEnd` in that order); `create_rolls_back_conversation_when_message_insert_fails` (in `postgres_transactions.rs`, trigger-injected failure on `messages` insert, assert zero `conversations` rows remain).
- [ ] **Step 2: Run** `cargo test -p services chat::conversations` — Expected: FAIL (compile).
- [ ] **Step 3: Implement** per the behaviour above; delete the old `create`.
- [ ] **Step 4: Run** `cargo test --workspace`, then `cargo test -p services --no-fail-fast --tests -- --ignored` — Expected: PASS except the known-bug rows already listed in the testing spec; no new failures.
- [ ] **Step 5: Commit** `feat: create a conversation from its first message`.

---

### Task 6: HTTP layer

**Files:**
- Modify: `api/src/endpoints/conversations.rs` (`create`, `sse`, utoipa descriptions)

**Interfaces:**
- Consumes: Task 5 `create`, Task 3 `into_events`.
- Produces: `POST /conversations` with `request_body = ExchangeCreate` returning `text/event-stream` (200), errors as JSON before the stream starts, same shape as `create_exchange`.

- [ ] **Step 1: Rewrite `create`** to take `Json(body): Json<ExchangeCreate>`, call `services::chat::create(body, &user, &state.conn, state.registry.as_ref(), state.jobs.as_ref(), state.clock.as_ref())` and answer like `create_exchange` does. Update its `#[utoipa::path]` (200 `text/event-stream` body `ExchangeEvent`, 400 for `ExchangeContentEmpty`, 401, 500).
- [ ] **Step 2: Rewrite `sse`** as `subscription.into_events().map(|event| Ok(Event::default().event(event.name()).json_data(&event)...))` keeping the existing fallback for a serialization error and `KeepAlive::default()`. Drop the hand-rolled `unfold`.
- [ ] **Step 3: Update the descriptions** of `create`, `create_exchange`, `stream` and `retry` to list the events and say every stream ends with `stream_end`; `create` also lists `conversation_created` first and `title` before `stream_end`.
- [ ] **Step 4: Run** `cargo build && cargo test --workspace && cargo clippy --workspace --all-targets` — Expected: PASS, no new warnings.
- [ ] **Step 5: Manual check** (needs a `config.yaml` with `openrouter.title_model` and a key): `cargo run -- serve`, then `curl -N -X POST localhost:3000/conversations -H 'Authorization: Bearer <token>' -H 'Content-Type: application/json' -d '{"content":"What should I watch tonight?"}'`. Expected: `conversation_created`, text deltas, `exchange_completed`, `title`, `stream_end`, then the connection closes; `GET /conversations` shows the title.
- [ ] **Step 6: Commit** `feat: stream the first exchange from POST /conversations`.

---

### Task 7: Documentation and final verification

**Files:**
- Modify: `CONTEXT.md`, `docs/superpowers/specs/2026-10-29-testing-infrastructure-design.md` (only if a known-bug row is needed)

- [ ] **Step 1: `CONTEXT.md`:** change the **Conversation** line to say it is created with its first **Exchange**; under Relationships keep the title line and add "A Conversation's title is delivered on the stream of the request that created it; other streams never carry it."; add a **Stream end** term: "The last event of every SSE stream the server ends; it says nothing more will come, not that the run succeeded."
- [ ] **Step 2: Run** `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo test -p services --no-fail-fast --tests -- --ignored` — Expected: all pass; ignored suite shows only the pre-existing known-bug failures.
- [ ] **Step 3: Commit** `docs: describe conversation titles and stream_end`.
