# Exchange Retry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A person can retry any failed Exchange: its trailing assistant Message is discarded, the Exchange moves to the end of the Conversation, and the model is called again from the Messages that succeeded.

**Architecture:** `retry_exchange` (services) flips the Exchange back to `in_progress` and deletes the trailing assistant Message in one transaction, seeds the registry with the kept blocks, and submits the existing `Job::RunExchange`. Exchanges sort by `(created_at, id)`, and Retry sets `created_at` to now, which moves the Exchange last. The run derives its tool round budget from stored Tool Messages.

**Tech Stack:** Rust workspace, Axum, SeaORM 1.1 (Postgres), tokio, mockall fakes in `services::test_support`.

**Spec:** `docs/superpowers/specs/2026-11-02-exchange-retry-design.md`. Domain terms: `CONTEXT.md`. Rules for services code and tests: `CLAUDE.md`.

## Global Constraints

- No migration; no new `Job` variant; no new SSE event type; no new DTO.
- Timestamps only from `Clock`; background work only through `Jobs`; no `tokio::spawn`, `Utc::now` or `std::fs` in `services/src`.
- Parameter order for services functions: own parameters, then `db, registry, jobs, clock` (each only where needed).
- Fixed query count; no per-item queries.
- `llm_calls` rows are never deleted or modified by Retry.
- Unit tests in `#[cfg(test)] mod tests` of the file under test; names `<function>_<expected outcome>_when_<condition>`; call `test_support::init()` first; never assert full SQL strings; Postgres tests are `#[ignore = "requires docker"]` and only for what the mock DB cannot prove.
- New builders go in `services::test_support::fixtures` or `services/tests/common/seed.rs`, never in a test module.
- After every task: `cargo test --workspace` and `cargo clippy --workspace --all-targets` add no failures or warnings.
- Commit messages end with the `Co-Authored-By` trailer from the session's attribution rule.

## Review Focus

Inputs the spec implies but no happy-path test covers, most likely first. Each has its test in the named task.

- The failed Exchange's last Message is an assistant `tool_use` whose tool round never ran: it must be deleted, or the model rejects the history. (Task 6, `retry_exchange_deletes_unanswered_tool_use_message_when_tool_round_failed`)
- Two Retry clicks in a row, or a Retry racing another Exchange: only one run starts and no state is half-written. (Task 4 `…no_row`; Task 6 `…another_exchange_is_running`, `…rolls_back…`)
- Retry arrives while the old run's registry entry is still open after its terminal commit: rejected, database untouched. (Task 4)
- The Exchange failed before any output: no kept blocks, the stream replays nothing and goes live. (Task 4)
- Retry after `ToolRoundLimit` must not restore the tool budget. (Task 3)

---

### Task 1: Order Exchanges by `(created_at, id)`

**Files:**
- Modify: `services/src/services/chat/conversations.rs` (`details`, the Exchanges query)
- Modify: `services/src/services/chat/exchange.rs` (`load_history`)
- Test: both files' `tests` modules

**Interfaces:**
- Produces: both queries sort Exchanges by `created_at` ascending, then `id` ascending. Messages keep ordering by `id`.

- [ ] **Step 1: Create the branch** `git switch -c exchange-retry` (master is the default branch).
- [ ] **Step 2: Write the failing tests.** In `conversations.rs`: `details_orders_exchanges_by_created_at_then_id`, queueing an empty Conversation's results (`conversation_model(3, 1)`, an empty Exchange list, an empty Message list) and asserting the statement containing `FROM "exchanges"` contains `ORDER BY "exchanges"."created_at" ASC, "exchanges"."id" ASC`. In `exchange.rs`: `load_history_orders_exchanges_by_created_at_then_id`, calling `load_history(3, &db)` with two empty results and the same assertion on `log[0].sql`.
- [ ] **Step 3: Run** `cargo test -p services orders_exchanges_by_created_at_then_id` and `load_history_orders`. Expected: FAIL (ordered by id only).
- [ ] **Step 4: Implement:** add `.order_by_asc(exchanges::Column::CreatedAt)` before the existing `Id` ordering in both queries.
- [ ] **Step 5: Run** `cargo test --workspace`. Expected: PASS.
- [ ] **Step 6: Commit** `Order exchanges by created_at then id`.

### Task 2: Registry entries that start with blocks

**Files:**
- Modify: `services/src/services/chat/registry.rs`
- Test: same file

**Interfaces:**
- Produces: `ExchangeRegistry::open_with(&self, exchange_id: i32, blocks: Vec<DisplayBlock>)` (like `open`, keeps an existing entry; the blocks fill slots `0..n`), and `ExchangeRegistry::is_open(&self, exchange_id: i32) -> bool`.

- [ ] **Step 1: Write failing tests:**
  - `open_with_replays_seeded_blocks_then_live_when_subscribed`: seed `[Text "Hi", Tool(call)]`; `subscribe` replays `ExchangeEvent::replay(0, ..)`, `replay(1, ..)` and has a live receiver.
  - `open_with_sends_no_duplicate_when_event_published_after_subscribe`: subscribe, publish a delta for block 2; the receiver gets only that event, and a second subscriber replays all three blocks.
  - `is_open_returns_false_when_entry_closed`: true after `open_with`, false after `close`.
- [ ] **Step 2: Run** `cargo test -p services registry`. Expected: FAIL (methods missing).
- [ ] **Step 3: Implement** both methods in `registry.rs`; `open` becomes `open_with(id, Vec::new())`.
- [ ] **Step 4: Run** `cargo test -p services registry`. Expected: PASS.
- [ ] **Step 5: Commit** `Open registry entries with blocks`.

### Task 3: Derive the tool round budget from stored Messages

**Files:**
- Modify: `services/src/services/chat/exchange.rs` (`Run::exchange`)
- Test: same file

**Interfaces:**
- Produces: private `fn spent_rounds(messages: &[messages::Model], max: u32) -> (u32, bool)`: with `n` Tool Messages returns `(min(n, max), n > max)`. `Run::exchange` sets `self.rounds` and `self.refused` from it for `history[current].1` and `config().assistant.max_tool_rounds`, right after `current` is found.

- [ ] **Step 1: Write failing tests** (test config has `max_tool_rounds: 2`):
  - `spent_rounds_returns_zero_when_exchange_has_no_tool_message`, `spent_rounds_counts_tool_messages_when_below_limit` (1 → `(1, false)`), `spent_rounds_is_not_refused_when_at_limit` (2 → `(2, false)`), `spent_rounds_is_refused_when_tool_messages_exceed_limit` (3 → `(2, true)`).
  - `run_exchange_fails_with_tool_round_limit_when_stored_tool_messages_exceed_limit`: build the mock DB by hand like `run_exchange_loads_conversation_owner_for_tools_when_tool_round_runs`, but with the history query returning `[user, (assistant tool_use, tool result) × 3]`; then queue the LLM call insert, the assistant Message insert and two exec results. The model answers `tool_use_answer(&[date_time_call("tu_9")], StopReason::ToolUse)`. Expect events `[failed("ToolRoundLimit", "the model asked for tools again after reaching the tool round limit")]`.
  - `run_exchange_numbers_new_blocks_after_stored_blocks_when_exchange_resumed` (pins existing behaviour, so it may pass at once): the history query returns `[user, assistant [text "Let me check", tool_use], tool result with a `mia_display` for `date_time`]` (2 stored blocks); the model answers `text_answer(&["It is noon"], StopReason::EndTurn)`; expect events `[delta(2, "It is noon"), Completed]`.
- [ ] **Step 2: Run** `cargo test -p services spent_rounds run_exchange_fails_with_tool_round_limit_when_stored`. Expected: FAIL (the resumed-numbering test may already pass).
- [ ] **Step 3: Implement** `spent_rounds` and call it in `Run::exchange`.
- [ ] **Step 4: Run** `cargo test -p services`. Expected: PASS (all earlier tool round tests unchanged, since `n = 0` there).
- [ ] **Step 5: Commit** `Derive tool round budget from stored messages`.

### Task 4: `retry_exchange` service

**Files:**
- Modify: `services/src/infrastructure/rule_violation.rs` (add `ExchangeNotFailed` after `ExchangeContentEmpty`)
- Modify: `services/src/services/chat/conversations.rs`
- Test: same file

**Interfaces:**
- Consumes: `ExchangeRegistry::open_with` / `is_open` (Task 2); existing `find_conversation`, `find_exchange`, `violation`, `conflict_or_db`; `display_blocks`.
- Produces: `pub async fn retry_exchange(conversation_id: i32, exchange_id: i32, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry, jobs: &dyn Jobs, clock: &dyn Clock) -> Result<ExchangeSubscription, SrvErr>`, re-exported by `services::chat` through the existing glob.

Behaviour, in order: `find_conversation`, `find_exchange` (404 otherwise); if `registry.is_open(exchange_id)` return `ConversationExchangeInProgress`; one transaction: conditional `UPDATE exchanges SET status=in_progress, error=NULL, completed_at=NULL, created_at=now WHERE id AND status=failed` (0 rows → rollback, `ExchangeNotFailed`; errors through `conflict_or_db`), then `DELETE FROM messages WHERE role='assistant' AND id = (SELECT max(id) FROM messages WHERE exchange_id=?)` (SeaORM `delete_many` with `in_subquery`, like `recover_interrupted`), then the Conversation's `updated_at` = now, then load the Exchange's remaining Messages ordered by `id`, commit. After commit: `registry.open_with(exchange_id, display_blocks(&kept))`, `registry.subscribe` (as `start_exchange` does, `Internal` if `None`), `jobs.submit(Job::RunExchange { exchange_id })`.

Mock queue order for success: queries `[conversation_model(3, 1)]`, `[exchange_model(7, 3, Failed)]`; execs `updated(1)` (Exchange), `deleted()` (Message), `updated(1)` (Conversation); then query `[kept messages]`.

- [ ] **Step 1: Write failing tests** (`RecordingJobs`, `fixed_clock`):
  - `retry_exchange_returns_not_found_when_conversation_of_other_user` (1 statement) and `…_when_exchange_of_other_conversation` (2 statements).
  - `retry_exchange_fails_with_exchange_in_progress_when_registry_entry_open`: `assert_single_violation`, statement count 2 (the lookups only), no `UPDATE`, `jobs.take()` empty.
  - `retry_exchange_fails_with_exchange_not_failed_when_update_matches_no_row`: exec `updated(0)`; no `DELETE FROM "messages"`, no `UPDATE "conversations"`, no job, `registry.is_open(7)` is false.
  - `retry_exchange_leaves_registry_closed_and_submits_no_job_when_delete_fails`: `append_exec_errors` on the delete; result is `Err`, entry not open, no job.
  - `retry_exchange_submits_job_and_replays_kept_blocks_when_exchange_failed`: kept `[user "Hi", assistant text "Hel"]`; replay is `[ExchangeEvent::replay(0, Text "Hel")]`, `live` is `Some`, jobs are `[Job::RunExchange { exchange_id: 7 }]`; `assert_wrote` shows the Exchange update (in-progress status value and the clock time) and the Conversation update (clock time); exactly one `DELETE FROM "messages"` runs, bound to the assistant role and Exchange 7.
  - `retry_exchange_replays_nothing_when_no_blocks_kept`: kept `[user "Hi"]`; `replay` empty, `live` `Some`.
- [ ] **Step 2: Run** `cargo test -p services retry_exchange`. Expected: FAIL to compile (missing function and variant).
- [ ] **Step 3: Implement** the variant and `retry_exchange` per the behaviour above; put the transaction in a private `reopen_exchange(...) -> Result<Vec<messages::Model>, SrvErr>` beside `insert_exchange`.
- [ ] **Step 4: Update `CONTEXT.md`:** append to **Retry**: "The Exchange also moves to the end of the **Conversation**, and its creation time becomes the time of the Retry."
- [ ] **Step 5: Run** `cargo test --workspace` and `cargo clippy --workspace --all-targets`. Expected: PASS, no new warnings.
- [ ] **Step 6: Commit** `Add retry_exchange`.

### Task 5: Retry endpoint

**Files:**
- Modify: `api/src/endpoints/conversations.rs`, `api/src/config/routes.rs`, `api/src/openapi.rs`

**Interfaces:**
- Consumes: `services::chat::retry_exchange` (Task 4), `ExchangeStreamParams`, the module's `sse` function.
- Produces: `pub async fn retry(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<ExchangeStreamParams>) -> Response`, `#[utoipa::path(post, operation_id = "conversations::retry", path = "/conversations/{conversation_id}/exchanges/{exchange_id}/retry", …)]` with the same responses as `stream` (200 `text/event-stream`, 400 mentioning `ExchangeNotFailed` and `ConversationExchangeInProgress`, 401, 404, 500). It calls `retry_exchange(.., &state.conn, state.registry.as_ref(), state.jobs.as_ref(), state.clock.as_ref())` and answers like `create_exchange`.

- [ ] **Step 1: Implement** the handler, add the route `post(conversations::retry)` next to the `stream` route, and add `crate::endpoints::conversations::retry` to the `paths(...)` list in `openapi.rs`.
- [ ] **Step 2: Run** `cargo build --workspace` and `cargo clippy --workspace --all-targets`. Expected: PASS, no new warnings. (The `api` crate has no tests; the service behaviour is covered in Tasks 4 and 6.)
- [ ] **Step 3: Commit** `Add retry endpoint`.

### Task 6: Postgres tests

**Files:**
- Modify: `services/tests/common/seed.rs` (add `pub async fn message(db: &DbConn, exchange_id: i32, role: MessageRole, content: serde_json::Value, llm_call_id: Option<i32>) -> messages::Model`)
- Modify: `services/tests/postgres_chat.rs`

Use `InlineJobs` with `JobDeps` as `start_exchange_persists_answer_and_ledger_when_stream_completes` does, and `with_recorded_answers` to read the requests sent. Use a clock later than `seed::chat_time()` (2024-01-01 12:00:00), for example `fixed_clock(2024, 1, 2, 12, 0, 0)`, since equal times fall back to id order.

- [ ] **Step 1: Write the tests** (all `#[ignore = "requires docker"]`):
  - `retry_exchange_moves_exchange_behind_newer_one_and_completes_it_when_exchange_failed`: Exchange A (failed; user Message, an LLM call, a partial assistant Message) then Exchange B (completed, answered). After Retry: `details` lists `[B, A]`; A is `Completed` with `created_at` equal to the clock time; A's Messages are `[user, new assistant]`; the old LLM call row still exists; the request sent to the model ends with A's user text, after B's two Messages.
  - `retry_exchange_deletes_only_trailing_assistant_message_when_partial_follows_tool_round`: A holds `[user, assistant tool_use, tool result, partial assistant]`; the request has 3 Messages (the last carries the `tool_result`) and the first three stored Messages are untouched.
  - `retry_exchange_deletes_unanswered_tool_use_message_when_tool_round_failed`: A holds `[user, assistant tool_use]`; the request has 1 Message (the user's) and the `tool_use` Message is gone.
  - `retry_exchange_fails_with_exchange_in_progress_when_another_exchange_is_running`: A failed, B `in_progress`; the result is the `ConversationExchangeInProgress` violation (the unique index), and A is still failed with its partial Message and old `created_at`.
  - `retry_exchange_rolls_back_status_and_message_delete_when_conversation_update_fails`: `fail_on(&db, "conversations", "UPDATE")`; the result is `Err`, A is still failed, its partial Message exists, `created_at` is unchanged, no job ran (a `MockLlm` with no expectations).
- [ ] **Step 2: Run** `cargo test -p services --test postgres_chat -- --ignored`. Expected: all PASS.
- [ ] **Step 3: Run the whole verification** `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo test -p services --no-fail-fast --tests -- --ignored`. Expected: only the existing `known bug:` tests fail.
- [ ] **Step 4: Commit** `Test exchange retry against Postgres`.
