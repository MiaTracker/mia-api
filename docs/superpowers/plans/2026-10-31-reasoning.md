# Reasoning in Conversations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Request reasoning from the model when `openrouter.reasoning.budget_tokens` is configured, stream and show thinking as Exchange blocks and strip earlier Exchanges' thinking from history. Failure handling is unchanged: thinking without text stays `EmptyResponse`.

**Architecture:** The config only shapes the request built by `OpenRouterClient`. Everything else is unconditional: thinking becomes a second displayable block kind sharing the Exchange-wide block numbering — the accumulator emits `ThinkingDelta` events alongside `TextDelta`, `display_blocks` lists thinking blocks, the registry and the DB replay re-emit them by kind. `history::build_request` strips thinking from earlier Exchanges.

**Tech Stack:** Rust workspace, openrouter-rs 0.22 (unchanged), SeaORM mock DB, mockall `MockLlm`, Axum SSE, utoipa.

**Spec:** `docs/superpowers/specs/2026-10-31-reasoning-design.md` (builds on `docs/superpowers/specs/2026-10-29-llm-chat-endpoints-design.md`; terms in `CONTEXT.md`).

## Global Constraints

- Stay on openrouter-rs **0.22**. No fork, no `[patch]`, no workaround for the `signature`-less thinking block gap.
- Config: optional `openrouter.reasoning.budget_tokens`; absent → `thinking: disabled`. No `effort`. Startup fails if `budget_tokens >= max_tokens`.
- Display and history stripping never look at the config. No new failure key; `Run::outcome` is unchanged.
- Wire names: SSE event `thinking_delta` with data `{"block": <u32>, "text": <string>}`; display block `{"type": "thinking", "thinking": <string>}`.
- Write to `messages.content` before publishing any delta (thinking included). `signature_delta` is stored, never forwarded.
- `redacted_thinking` stays ignored by the accumulator.
- No migration.
- `CLAUDE.md` rules: test names `<function>_<expected outcome>_when_<condition>`, `test_support::init()` first, builders in `test_support` (never local), ignore reasons exactly as listed, parameter order unchanged.
- After every task: `cargo test --workspace` and `cargo clippy --workspace --all-targets` with no new warnings.

## Review Focus

1. A rejoin while thinking is still streaming must replay the thinking as `thinking_delta` (not `text_delta`) and continue live — registry test in Task 2.
2. A stream that fails mid-thinking keeps the thinking as the Partial message and the client saw it as `thinking_delta` before `exchange_failed` — exchange test in Task 2.
3. A buffered thinking + text answer coalesced into one write publishes two events in block order, thinking (0) then text (1) — exchange test in Task 2.
4. A thinking block that only ever gets a signature or empty deltas gets no block number and no event — accumulator test in Task 2.
5. An earlier completed Exchange whose stored assistant Message has thinking with an empty signature sends no thinking block at all — history test in Task 3.

---

### Task 1: Configure reasoning and request it

**Files:**
- Modify: `infrastructure/src/config.rs` (`OpenRouterConfiguration`, `load_config`)
- Modify: `integrations/src/openrouter/client.rs` (`OpenRouterClient::new`, `build_request`, tests)
- Modify: `api/src/lib.rs:47`
- Modify: `services/src/test_support/config.rs:13`
- Modify: `integrations/tests/openrouter_contract.rs`
- Modify: `config.yaml.example`, `CLAUDE.md` (config block)

**Interfaces:**
- Produces: `pub struct ReasoningConfiguration { pub budget_tokens: u32 }`; `OpenRouterConfiguration.reasoning: Option<ReasoningConfiguration>` (`#[serde(default)]`); `impl OpenRouterConfiguration { pub fn validate(&self) -> Result<(), String> }`; `OpenRouterClient::new(api_key: String, model: String, max_tokens: u32, thinking_budget: Option<u32>) -> Self`.

- [ ] **Step 0: Branch and commit the spec**

```bash
git checkout -b reasoning
git add docs/superpowers/specs/2026-10-31-reasoning-design.md docs/superpowers/plans/2026-10-31-reasoning.md
git commit -m "Add reasoning design and plan"
```

- [ ] **Step 1: Write the failing tests**

In `infrastructure/src/config.rs`, new `#[cfg(test)] mod tests`:
- `validate_fails_when_budget_tokens_not_below_max_tokens`: `max_tokens: 4096, reasoning: Some(ReasoningConfiguration { budget_tokens: 4096 })` → `validate().is_err()`.
- `validate_succeeds_when_budget_below_max_tokens`: budget 2048, max 4096 → `Ok(())`.
- `validate_succeeds_when_reasoning_absent`: `reasoning: None` → `Ok(())`.
- `openrouter_configuration_reads_budget_tokens_when_reasoning_present`: `serde_yaml::from_str` of `"api_key: k\nmodel: m\nreasoning:\n  budget_tokens: 8000\n"` → `reasoning.unwrap().budget_tokens == 8000`, `max_tokens == 16384`.

In `integrations/src/openrouter/client.rs` tests (serialize the built request with `serde_json::to_value`):
- `build_request_disables_thinking_when_no_budget`: `value["thinking"] == json!({"type": "disabled"})`.
- `build_request_enables_thinking_with_budget_when_budget_set`: `OpenRouterClient::new(.., 4096, Some(2048))` → `value["thinking"] == json!({"type": "enabled", "budget_tokens": 2048})`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p infrastructure config && cargo test -p integrations build_request`
Expected: compile errors (`ReasoningConfiguration`, 4-argument `new` don't exist).

- [ ] **Step 3: Implement**

- `ReasoningConfiguration` + `reasoning` field; `validate` returns `Err` with a message naming both values when `budget_tokens >= max_tokens`. `load_config` calls `validate` after deserializing and `fail!`s on `Err`.
- `OpenRouterClient` stores `thinking_budget: Option<u32>`; `build_request` uses `AnthropicThinking::Enabled { budget_tokens }` when set, `Disabled` otherwise.
- `api/src/lib.rs` passes `openrouter.reasoning.as_ref().map(|r| r.budget_tokens)`.
- `test_support/config.rs`: `reasoning: None`.
- Existing contract test: pass `None`. Add `stream_message_reaches_stop_reason_when_reasoning_enabled` (`#[ignore = "requires OpenRouter key"]`, `openrouter/free`, `max_tokens 2048`, `Some(1024)`, same short prompt): every item `Ok` (`.expect("stream yielded an error")`), some `MessageDelta { stop_reason: Some(_) }`; print the number of non-empty `ThinkingDelta`s with `eprintln!`. Do not assert that thinking appears. Do not tolerate `unparseable_response`.
- `config.yaml.example` and the `CLAUDE.md` config block gain:

```yaml
  reasoning:            # optional; absent disables reasoning
    budget_tokens: 8000 # must be below max_tokens
```

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no new warnings.

- [ ] **Step 5: Run the live contract tests**

Run: `cargo test -p integrations --test openrouter_contract -- --ignored --nocapture`
Expected: both pass. If the reasoning test fails with an `unparseable_response` / `Serialization error`, that is the known gap: leave the test as written, record the failure output for the PR, and continue (user's decision). Any other failure (e.g. 400 on the `thinking` parameter) — stop and report.

- [ ] **Step 6: Commit**

```bash
git add -A infrastructure integrations api services/src/test_support/config.rs config.yaml.example CLAUDE.md
git commit -m "Request reasoning when openrouter.reasoning.budget_tokens is configured"
```

---

### Task 2: Thinking as a block, live and on reload

One task because the shared numbering rule spans all of it: `display_blocks`, the accumulator, the registry and both replay paths must change together or the stream and the view disagree (and `ExchangeEvent`'s exhaustive matches stop compiling).

**Files:**
- Modify: `views/src/conversations/conversations.rs` (`DisplayBlock`, `ExchangeEvent`, tests)
- Modify: `services/src/services/chat/display.rs`
- Modify: `services/src/services/chat/accumulator.rs`
- Modify: `services/src/services/chat/exchange.rs` (`read`, `merge`, tests)
- Modify: `services/src/services/chat/registry.rs`
- Modify: `services/src/services/chat/conversations.rs:122-130` (finished-Exchange replay)
- Modify: `services/src/test_support/llm.rs`

**Interfaces:**
- Produces: `DisplayBlock::Thinking { thinking: String }` (serialized `{"type": "thinking", "thinking": ...}`); `ExchangeEvent::ThinkingDelta { block: u32, text: String }` with `name() == "thinking_delta"`; `impl ExchangeEvent { pub fn replay(block: u32, shown: DisplayBlock) -> Self }` mapping `Text` → `TextDelta`, `Thinking` → `ThinkingDelta`.
- `display_blocks(&[messages::Model]) -> Vec<DisplayBlock>` keeps its signature; now also yields non-empty thinking blocks in content order.
- `Accumulator::apply(&mut self, event: LlmEvent) -> Vec<ExchangeEvent>` (only `TextDelta` / `ThinkingDelta`); `fn merge(deltas: Vec<ExchangeEvent>) -> Vec<ExchangeEvent>` in `exchange.rs`.
- `test_support::llm::thinking_then_text_answer(thinking: &str, chunks: &[&str], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>>` (thinking block index 0, text block index 1).

- [ ] **Step 1: Write / update the failing tests**

`views`:
- `exchange_event_serializes_block_and_text_when_thinking_delta`: `to_value(ThinkingDelta { block: 0, text: "Hm" }) == json!({"block": 0, "text": "Hm"})`, `name() == "thinking_delta"`.
- `display_block_serializes_with_thinking_type_when_thinking`: `json!({"type": "thinking", "thinking": "Hm"})`.
- `replay_returns_thinking_delta_when_block_is_thinking`.

`display.rs`:
- Replace `display_blocks_skips_non_assistant_messages_and_empty_text_when_stored` with `display_blocks_includes_thinking_and_skips_empty_blocks_when_stored`: same messages plus `{"type":"thinking","thinking":"","signature":"s"}` in the assistant content; expect `[Thinking { "hmm" }, Text { "answer" }]`.
- `display_blocks_matches_stream_numbering_when_content_reloaded`: collect streamed `ExchangeEvent`s per block into `DisplayBlock`s by kind; expected `[Thinking "hmm", Text "Hello", Text "World"]`.

`accumulator.rs` (existing tests switch from `(u32, String)` to `ExchangeEvent`):
- `apply_returns_thinking_delta_with_block_when_thinking_delta`: `ThinkingDelta{index 0,"hmm"}` → `[ExchangeEvent::ThinkingDelta { block: 0, text: "hmm" }]`.
- Rename `apply_numbers_second_text_block_next_when_provider_index_skips_thinking` → `apply_numbers_text_after_thinking_when_thinking_came_first`: the text delta gets `block: 1`.
- `apply_returns_nothing_when_thinking_block_gets_only_signature_or_empty_deltas`: `BlockStart Thinking`, `ThinkingDelta ""`, `SignatureDelta "s"` → each `apply` returns `[]`; a following text block gets `block: 0`.

`exchange.rs`:
- Update `run_exchange_fails_with_empty_response_when_only_thinking_arrived`: events `[ThinkingDelta{0,"hmm"}, failed("EmptyResponse", ..)]`.
- `run_exchange_publishes_thinking_then_text_in_block_order_when_buffered`: `with_answer(thinking_then_text_answer("hmm", &["Hi"], EndTurn))` → events `[ThinkingDelta{0,"hmm"}, TextDelta{1,"Hi"}, Completed]`; one `INSERT INTO "messages"` with `[{"type":"thinking","thinking":"hmm","signature":""},{"type":"text","text":"Hi"}]`.
- `run_exchange_keeps_thinking_as_partial_message_when_stream_fails_mid_thinking`: thinking events then `Err(stream_error("provider_overloaded", "overloaded"))` → `[ThinkingDelta{0,"hmm"}, failed("ProviderUnavailable", "provider_overloaded: overloaded")]`; the message insert holds the thinking.

`registry.rs`:
- `subscribe_replays_thinking_as_thinking_delta_when_joining_mid_thinking`: publish `ThinkingDelta{0,"hm"}`, `ThinkingDelta{0,"m"}`, then subscribe → replay `[ThinkingDelta{0,"hmm"}]`, live present; then publish `TextDelta{1,"Hi"}` → live receives it.

`conversations.rs`:
- `subscribe_replays_thinking_then_text_when_exchange_finished` (match the existing finished-replay test's setup): stored assistant content `[thinking "hmm", text "Hi"]`, Exchange completed → replay `[ThinkingDelta{0,"hmm"}, TextDelta{1,"Hi"}, Completed]`.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p views && cargo test -p services chat`
Expected: compile errors, then assertion failures.

- [ ] **Step 3: Implement**

- Views: add the variants and `replay`.
- `display.rs`: the display path matches each block's `type` (`"text"` → `Text`, `"thinking"` → `Thinking`, empty skipped); `user_text` keeps using text blocks only.
- Accumulator: `Body::Thinking` gains `block: Option<u32>`, assigned from the shared `next_block` counter on its first non-empty thinking delta, exactly as text does. Signature deltas never number or emit.
- `read` publishes the merged events directly; `merge` joins deltas of the same block (a block has one kind, so the block number is the key).
- Registry entry keeps each block's kind (e.g. `Vec<Option<DisplayBlock>>`, builder's choice); `publish` appends to the matching block; `subscribe` replays via `ExchangeEvent::replay`.
- `conversations.rs` DB replay uses `ExchangeEvent::replay`.
- Add `thinking_then_text_answer` to `test_support::llm`.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS, no new warnings.

- [ ] **Step 5: Commit**

```bash
git commit -am "Show and stream thinking as blocks with Exchange-wide numbering"
```

---

### Task 3: Strip earlier Exchanges' thinking from history

**Files:**
- Modify: `services/src/services/chat/history.rs`

**Interfaces:**
- `build_request(current_exchange_id: i32, exchanges: &[(exchanges::Model, Vec<messages::Model>)]) -> LlmRequest` keeps its signature.

- [ ] **Step 1: Write the failing tests**

- `build_request_strips_thinking_when_exchange_is_earlier`: completed Exchange 1 assistant content `[{"type":"thinking","thinking":"hmm","signature":""}, {"type":"redacted_thinking","data":"x"}, {"type":"text","text":"reply"}]`; current Exchange 2 user "two" → message 1 content `== json!([{"type":"text","text":"reply"}])`; stored model untouched.
- `build_request_keeps_thinking_when_exchange_is_current`: current Exchange 1 has user "q" and assistant `[thinking "hmm", text "a"]` → assistant content still starts with the thinking block (`content[0]["type"] == "thinking"`).
- `build_request_drops_message_when_stripping_leaves_it_empty`: completed Exchange 1 assistant content only thinking → request has user "one" then current user "two", 2 messages.
- `build_request_marks_last_block_after_stripping`: last message is from the current Exchange; assert marker on its last block and none on the stripped earlier message's remaining text block.

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p services build_request`
Expected: FAIL on the strip and drop tests.

- [ ] **Step 3: Implement**

For messages of Exchanges other than `current_exchange_id`, filter out blocks whose `type` is `thinking` or `redacted_thinking`; drop a message only when stripping emptied a non-empty array. Add the cache marker after stripping. Update the doc comment.

- [ ] **Step 4: Run tests**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git commit -am "Strip earlier Exchanges' thinking from the model history"
```

---

### Task 4: Final verification

- [ ] **Step 1:** `cargo test --workspace && cargo clippy --workspace --all-targets` — PASS, no new warnings.
- [ ] **Step 2:** `cargo test -p services --test postgres_chat -- --ignored` — PASS (end-to-end run against real Postgres still works with the new event set).
- [ ] **Step 3:** `cargo test -p integrations --test openrouter_contract -- --ignored --nocapture` — record the result, including any known-gap failure and the printed thinking-delta count, for the PR description.
