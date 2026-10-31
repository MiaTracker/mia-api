# Chat Tools Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the chat assistant four tools (`mia_search`, `mia_details`, `tmdb_search`, `date_time`), run tool rounds inside an Exchange with a round limit, and show each tool call to the person as a display block.

**Architecture:** `integrations::openrouter` learns tool definitions, `tool_use` block starts, `input_json_delta` and `redacted_thinking`. The services accumulator stores `tool_use` blocks; `run_exchange` loops over LLM calls, running tools between them and storing one `tool` Message per round whose `tool_result` blocks carry a mia-only `mia_display` field. The display rule reads that field to build `tool` blocks, and `history::build_request` strips it before anything reaches the model. Tool code lives in `services/src/services/chat/tools/` and reuses search helpers extracted from `media::search`.

**Tech Stack:** Rust workspace, axum SSE, SeaORM (mock DB for unit tests, testcontainers Postgres), openrouter-rs 0.22, mockall.

**Spec:** `docs/superpowers/specs/2026-10-31-chat-tools-design.md` (read it with this plan; terms from `CONTEXT.md`).

## Global Constraints

- Follow `CLAUDE.md` exactly: parameter order `db, llm, registry, tmdb, store, jobs, clock, random`; no `Utc::now` / `tokio::spawn` / `std::fs` in `services/src`; tests named `<function>_<expected outcome>_when_<condition>`; `test_support::init()` first; builders only in `test_support`; `SrvErr` has no `Debug`.
- `media::search`, `movies::details`, `series::details`: public signatures and behaviour unchanged.
- No n+1 queries: anything handling a list of ids issues a fixed number of queries independent of the list's length (user rule: minimizing database calls is a project goal). Pin it with `assert_statement_count`.
- Tools never call `transpiler::transpile` or `Tmdb::search_multi` themselves; they go through the helpers of Task 4.
- Nothing model-specific: no `tool_choice`, tool definitions sent as plain `name` / `description` / `input_schema` (openrouter-rs `tool_type` set to `None`).
- Config (optional section, defaults): `assistant.mia_search_limit: 30`, `assistant.mia_details_limit: 60`, `assistant.max_tool_rounds: 8`; each must be ≥ 1.
- Failure keys added: `OutputTruncated`, `ToolRoundLimit`.
- SSE event names: `text_delta`, `thinking_delta`, `tool_call`, `exchange_completed`, `exchange_failed`.
- Round-limit refusal text, exactly: `tool limit reached; answer with what you have`.
- `date_time` answers in UTC, ISO 8601, labelled UTC.
- After each task: `cargo test --workspace` and `cargo clippy --workspace --all-targets` (no new warnings). Task 4 also runs `cargo test -p services --no-fail-fast --tests -- --ignored` (Postgres search suite must pass unchanged, apart from known-bug tests that already fail).

## Review Focus

- A Conversation deleted while a tool round runs: the tool Message insert fails on the foreign key; the run must stop quietly as `Deleted` (no `exchange_failed`, `llm_calls` closed with the deleted error), like a deleted mid-stream run. Test in Task 10. *(Superseded by the Task 10 ruling: the round's LLM call was already closed successfully before the tools ran and is not re-closed with the deleted error.)*
- `mia_details` with repeated ids (`[5, 5]`): each media appears once in the result and once in the block's titles. Test in Task 8.
- `mia_details` with `ids: []`: error result `ids must not be empty`, not an empty success. Test in Task 8.
- `tmdb_search` with a `type` other than `movie` / `series` (e.g. `"tv"`): error result naming the allowed values, no TMDB call. Test in Task 9.
- `mia_search` with an empty query `""`: a normal result listing the first `mia_search_limit` media of the library, not an error. Test in Task 6.

---

### Task 1: OpenRouter tool types and translation

**Files:**
- Modify: `integrations/src/openrouter/types.rs`, `integrations/src/openrouter/client.rs`, `integrations/src/openrouter/mod.rs`
- Modify: `services/src/services/chat/history.rs` (construct `LlmRequest` with `tools: Vec::new()` for now)
- Test: `integrations/src/openrouter/client.rs` (`mod tests`), `integrations/tests/openrouter_contract.rs`

**Interfaces:**
- Produces:
  - `pub struct LlmTool { pub name: String, pub description: String, pub input_schema: serde_json::Value }` (Clone, Debug, PartialEq), re-exported from `integrations::openrouter`.
  - `LlmRequest { system, messages, pub tools: Vec<LlmTool> }`.
  - `BlockKind::ToolUse { id: String, name: String }`, `BlockKind::RedactedThinking { data: String }` (added before `Other`).
  - `LlmEvent::InputJsonDelta { index: u32, partial_json: String }`.

- [ ] **Step 1: Write the failing tests** in `client.rs` `mod tests`:
  - `translate_event_returns_tool_use_block_start_when_tool_use_block_starts` — `content_block_start` with `{"type":"tool_use","id":"tu_1","name":"date_time","input":{}}` → `[BlockStart { index: 1, kind: BlockKind::ToolUse { id: "tu_1", name: "date_time" } }]` (empty initial input emits no delta).
  - `translate_event_returns_input_json_delta_when_tool_use_block_starts_with_input` — initial `input: {"query":"x"}` → `BlockStart` then `InputJsonDelta { partial_json: r#"{"query":"x"}"# }`.
  - `translate_event_returns_input_json_delta_when_delta_is_input_json` — delta `{"type":"input_json_delta","partial_json":"{\"q"}` → `InputJsonDelta { index, partial_json: "{\"q" }`.
  - `translate_event_returns_redacted_thinking_block_start_when_redacted_thinking_starts` — replaces `translate_event_returns_other_block_start_when_block_type_unhandled`'s redacted case: `{"type":"redacted_thinking","data":"x"}` → `BlockKind::RedactedThinking { data: "x" }`. Keep an `Other` test with a different unhandled type (e.g. `server_tool_use`).
  - `translate_message_accepts_tool_use_and_tool_result_blocks` — assistant content `[{"type":"tool_use","id":"tu_1","name":"date_time","input":{}}]` and user content `[{"type":"tool_result","tool_use_id":"tu_1","content":"{\"now\":\"…\"}","is_error":true}]` both translate `Ok`.
  - `build_request_sends_tools_without_type_when_tools_given` — request JSON (via the existing `request_json` helper, extended to pass tools) has `tools == [{"name":"date_time","description":"d","input_schema":{"type":"object"}}]` with no `type` key; with no tools the `tools` key is absent.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p integrations openrouter`
Expected: compile errors / FAIL (variants and `tools` field missing).

- [ ] **Step 3: Implement** the types above; in `build_request` add each tool with `builder.tool(tool)` where `tool = AnthropicTool::custom(name, description, input_schema)` then `tool.tool_type = None`; in `translate_block_start` map `ToolUse { id, name, input }` (emit `InputJsonDelta` with `input.to_string()` only when `input` is a non-empty object) and `RedactedThinking { data }`; in `translate_event` map delta type `input_json_delta` (field `partial_json`). Fix every `LlmRequest { … }` construction (history.rs, contract test, any test) with `tools: Vec::new()`.

- [ ] **Step 4: Add the live contract test** in `integrations/tests/openrouter_contract.rs`:
  `stream_message_yields_tool_use_with_parseable_input_when_tool_is_offered` (`#[ignore = "requires OpenRouter key"]`): system "Use the tool to answer.", user "What is the weather in Paris? Use the get_weather tool.", one tool `get_weather` with schema `{"type":"object","properties":{"city":{"type":"string"}},"required":["city"]}`; collect events; assert a `BlockStart { kind: ToolUse { name: "get_weather", .. } }` arrives and the concatenated `InputJsonDelta`s for that index parse as a JSON object. If `openrouter/free` routes to models without tool support and this proves flaky, pin one free tool-capable model for this test only and say so in a comment.

- [ ] **Step 5: Run unit tests, clippy, and the contract test**

Run: `cargo test -p integrations` then `cargo test -p integrations --test openrouter_contract -- --ignored`
Expected: PASS (contract: all three tests pass; a 429 is a failure, rerun later).

- [ ] **Step 6: Commit** — `git commit -m "Translate tool definitions, tool_use and redacted_thinking blocks for OpenRouter"`

---

### Task 2: Accumulator stores tool_use and redacted_thinking blocks

**Files:**
- Modify: `services/src/services/chat/accumulator.rs`
- Modify: `services/src/test_support/llm.rs` (builders)
- Test: `accumulator.rs` `mod tests`

**Interfaces:**
- Consumes: Task 1's `BlockKind::ToolUse`, `BlockKind::RedactedThinking`, `LlmEvent::InputJsonDelta`.
- Produces:
  - `pub struct ToolUse { pub id: String, pub name: String, pub input: Option<serde_json::Value> }` in `accumulator.rs` — `input` is `None` when the concatenated JSON does not parse to an object.
  - `Accumulator::tool_uses(&self) -> Vec<ToolUse>` — every tool_use block in order.
  - `Accumulator::tool_use_cut(&self) -> bool` — `stop_reason == Some(MaxTokens)` and some tool_use block has no `BlockStop` or unparseable input.
  - `content()` now also emits `{"type":"tool_use","id","name","input": <parsed object, or {} while unparsed/invalid>}` and `{"type":"redacted_thinking","data"}`, in provider order with the others.
  - `has_content()` is true for any tool_use or redacted_thinking block. `has_text()` unchanged. `apply` returns no deltas for tool_use / redacted_thinking (they are not numbered).
  - `test_support::llm::tool_use_answer(calls: &[(&str, &str, serde_json::Value)], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>>` — one tool_use block per `(id, name, input)` at indexes 0.., input streamed as one `InputJsonDelta` of `input.to_string()`.
  - `test_support::llm::text_then_tool_use_answer(chunks: &[&str], calls: &[(&str, &str, Value)], stop: StopReason)` — text block at index 0, tool_use blocks after.
  - `test_support::llm::with_answers(answers: Vec<Vec<Result<LlmEvent, LlmError>>>) -> MockLlm` — expects exactly `answers.len()` `stream_message` calls, answering them in order.

- [ ] **Step 1: Write the failing tests**
  - `content_includes_tool_use_with_parsed_input_when_block_stops` — events from `tool_use_answer(&[("tu_1","mia_search",json!({"query":"Alien"}))], ToolUse)` → `content() == json!([{"type":"tool_use","id":"tu_1","name":"mia_search","input":{"query":"Alien"}}])`, `tool_uses()[0].input == Some(json!({"query":"Alien"}))`.
  - `content_includes_tool_use_with_empty_input_when_input_is_not_an_object` — delta `"[1,2"` → stored `input: {}`, `tool_uses()[0].input == None`.
  - `apply_returns_no_deltas_and_numbers_nothing_when_tool_use_streams` — `Accumulator::new(3)`, tool_use then text: the text delta is `block 3`.
  - `has_content_is_true_and_has_text_false_when_only_tool_use_arrived`.
  - `tool_use_cut_is_true_when_max_tokens_stops_unfinished_tool_use` — tool_use with no `BlockStop`, stop `MaxTokens` → true; same with `BlockStop` and valid input → false; unparseable input with stop `EndTurn` → false.
  - `content_includes_redacted_thinking_when_block_starts` — `BlockStart { kind: RedactedThinking { data: "x" } }` → `[{"type":"redacted_thinking","data":"x"}]`, `has_content()` true, `has_text()` false.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p services accumulator` → FAIL.

- [ ] **Step 3: Implement** two new `Body` variants (`ToolUse { id, name, json: String, stopped: bool }`, `RedactedThinking { data }`); `BlockStop` marks a tool_use stopped; add the builders to `test_support::llm`.

- [ ] **Step 4: Run tests** — `cargo test -p services` → PASS (existing exchange tests unchanged).

- [ ] **Step 5: Commit** — `git commit -m "Accumulate tool_use and redacted_thinking blocks"`

---

### Task 3: Tool display blocks, `tool_call` event, `mia_display` stripping

**Files:**
- Modify: `views/src/conversations/conversations.rs`, `views/src/conversations/mod.rs`
- Modify: `services/src/services/chat/display.rs`, `services/src/services/chat/history.rs`, `services/src/services/chat/registry.rs`
- Modify: `services/src/services/chat/conversations.rs` (only if its replay needs changes; holds the replay test)
- Modify: `api/src/endpoints/conversations.rs` (SSE name comes from `ExchangeEvent::name`; update OpenAPI description of events if it lists them)
- Test: the `mod tests` of each modified services/views file

**Interfaces:**
- Produces:
  - `views::conversations::ToolCallView { pub name: String, pub query: Option<String>, pub media_type: Option<String>, pub count: Option<u32>, pub titles: Option<Vec<String>>, pub failed: bool }` — `Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq`; every `Option` is `#[serde(default, skip_serializing_if = "Option::is_none")]`.
  - `DisplayBlock::Tool(ToolCallView)` → serializes as `{"type":"tool","name":…, …}`.
  - `ExchangeEvent::ToolCall { block: u32, #[serde(flatten)] call: ToolCallView }`, `name() == "tool_call"`; `ExchangeEvent::replay(block, DisplayBlock::Tool(call))` → `ToolCall`.
  - `chat::display::DISPLAY_FIELD: &str = "mia_display"`.
  - `display_blocks(messages)` (signature unchanged) now walks assistant **and** tool Messages in order; for a tool Message each `tool_result` block with a `mia_display` object becomes `DisplayBlock::Tool` (deserialized from it); results without the field are skipped.
  - `history::build_request` removes `mia_display` from every `tool_result` block of every message sent (request copy only).
  - `ExchangeRegistry::publish(ToolCall)` stores `Some(DisplayBlock::Tool(call))` at `block` and forwards; `subscribe` replays it.

- [ ] **Step 1: Write the failing tests**
  - views: `exchange_event_serializes_block_and_call_fields_when_tool_call` → `{"block":2,"name":"mia_search","query":"Alien","count":3,"failed":false}`, name `tool_call`; `display_block_serializes_with_tool_type_when_tool` → `{"type":"tool","name":"date_time","failed":false}`; `replay_returns_tool_call_when_block_is_tool`.
  - display: `display_blocks_places_tool_blocks_after_their_message_blocks` — messages: user; assistant `[thinking "hm", tool_use tu_1, tool_use tu_2]`; tool `[tool_result tu_1 + mia_display{name:"mia_search",query:"Alien",count:2,failed:false}, tool_result tu_2 + mia_display{name:"date_time",failed:false}]`; assistant `[text "Done"]` → `[Thinking, Tool(mia_search…), Tool(date_time…), Text "Done"]`.
  - display: `display_blocks_skips_tool_use_when_no_tool_result_exists` — assistant `[text "Hi", tool_use]`, no tool Message → `[Text "Hi"]`.
  - history: `build_request_strips_display_field_from_tool_results` — the tool Message's block in the request has no `mia_display` key, keeps `tool_use_id` / `content` / `is_error`; the stored model is untouched.
  - registry: `subscribe_replays_tool_call_in_block_order_when_published` — publish `TextDelta{0}`, `ToolCall{1}`; subscribe → replay `[TextDelta{0}, ToolCall{1}]`, and a live subscriber receives a later `ToolCall`.
  - conversations: `subscribe_replays_tool_blocks_from_database_when_exchange_finished` — stored assistant + tool Messages of a completed Exchange replay as `ThinkingDelta`/`ToolCall`/`TextDelta` then `Completed`.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p views -p services` → FAIL.

- [ ] **Step 3: Implement** the interfaces above. Update every exhaustive `match` on `DisplayBlock` / `ExchangeEvent` (registry, exchange.rs `merge`/`delta_parts` ignore `ToolCall`).

- [ ] **Step 4: Run tests** — `cargo test --workspace` → PASS.

- [ ] **Step 5: Commit** — `git commit -m "Show tool calls as display blocks and tool_call events"`

---

### Task 4: Extract shared search helpers from `media::search`

**Files:**
- Modify: `services/src/services/media.rs`
- Test: `media.rs` `mod tests`; guard: existing `media::search` tests and `services/tests/postgres_search.rs`

**Interfaces:**
- Produces (in `media.rs`, `pub(crate)`):
  - `pub(crate) struct LibrarySearch { pub rows: Vec<(media::Model, Option<images::Model>, Option<titles::Model>)>, pub name_search: String, pub external_id: Option<transpiler::ExternalId>, pub is_primitive: bool }`
  - `pub(crate) async fn search_library(query: SearchQuery, page_req: PageReq, media_type: Option<MediaType>, user: &CurrentUser, db: &DbConn) -> Result<Result<LibrarySearch, Vec<String>>, SrvErr>` — transpiles and runs the query; `Err(messages)` holds one `"{source}: {message}"` string per `TranspilationError` (e.g. `"Construction: Unknown target 'foo'"`).
  - `pub(crate) enum TmdbHit { Movie(MultiMovieResult), Tv(MultiTvResult) }` with `pub(crate) fn tmdb_id(&self) -> i32` and `pub(crate) fn media_type(&self) -> MediaType`.
  - `pub(crate) async fn search_tmdb(query: String, media_type: Option<MediaType>, tmdb: &dyn Tmdb) -> Result<Vec<TmdbHit>, integrations::infrastructure::Error>` — `search_multi`, people and collections dropped, filtered to `media_type` when given, TMDB order kept. (Raw TMDB error so callers decide: `media::search` maps it with `?` as today; the tool turns it into an error result.)
- `media::search` rewritten on top of both, with identical output: poster sizes loaded in `search` itself, matched hits dropped, `external_limit` applied to kept hits.

- [ ] **Step 1: Write the failing tests**
  - `search_library_returns_transpile_messages_when_query_is_invalid` — query `" : foo = 1"` → `Ok(Err(messages))`, `messages == ["Construction: Unknown target 'foo'"]`, `assert_statement_count(&log, 0)`.
  - `search_tmdb_keeps_only_movies_in_tmdb_order_when_type_is_movie` — `MockTmdb` `search_multi` returns tv 1, movie 2, person 3, movie 4 → hits `[Movie 2, Movie 4]`.
  - `search_tmdb_drops_people_and_collections_when_no_type`.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p services media::tests::search_` → FAIL.

- [ ] **Step 3: Implement** the helpers and rewrite `media::search` on them. Do not touch any existing test.

- [ ] **Step 4: Run all suites**

Run: `cargo test --workspace` then `cargo test -p services --no-fail-fast --tests -- --ignored`
Expected: PASS everywhere except the known-bug tests that already failed before this task (compare with `git stash` run if unsure).

- [ ] **Step 5: Commit** — `git commit -m "Extract library and TMDB search helpers from media::search"`

---

### Task 5: Assistant config, tools module, `date_time`, dispatcher

**Files:**
- Modify: `infrastructure/src/config.rs`, `infrastructure/src/lib.rs` (re-export), `config.yaml.example`, `CLAUDE.md` (config block), `services/src/test_support/config.rs`
- Create: `services/src/services/chat/tools/mod.rs`, `services/src/services/chat/tools/date_time.rs`
- Modify: `services/src/services/chat/mod.rs` (`pub mod tools;`)
- Test: `config.rs` `mod tests` (create if absent), `tools/mod.rs` and `tools/date_time.rs` `mod tests`

**Interfaces:**
- Produces:
  - `infrastructure::AssistantConfiguration { pub mia_search_limit: u32, pub mia_details_limit: u32, pub max_tool_rounds: u32 }`, `Default` = 30 / 60 / 8; `Configuration.assistant: AssistantConfiguration` with `#[serde(default)]` (each field also `#[serde(default = …)]`); `AssistantConfiguration::validate(&self) -> Result<(), String>` (any field 0 → `Err("assistant.<field> must be at least 1")`), called in `load_config` next to `openrouter.validate()`.
  - Test config: `assistant: AssistantConfiguration { mia_search_limit: 30, mia_details_limit: 3, max_tool_rounds: 2 }` (small values so cap and limit tests stay short).
  - `tools::ToolOutcome { pub content: String, pub is_error: bool, pub display: ToolCallView }` — `content` is what the model reads (JSON text for successes, a plain message for errors).
  - `tools::definitions() -> Vec<LlmTool>` — the four tools (names `mia_search`, `mia_details`, `tmdb_search`, `date_time`), JSON Schemas per spec (`tmdb_search.type` is an `enum` of `movie`, `series`; `date_time` takes `{"type":"object","properties":{}}`).
  - `tools::run_tool(name: &str, input: &Value, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb, clock: &dyn Clock) -> Result<ToolOutcome, SrvErr>` — dispatches by name; `Err` only for DB failures.
  - `tools::failed(name: &str, input: &Value, message: &str) -> ToolOutcome` — `is_error: true`, `content: message`, display `{ name, query: input["query"] if string, media_type: input["type"] if string, failed: true }`.
  - `tools::result_block(tool_use_id: &str, outcome: &ToolOutcome) -> Value` — `{"type":"tool_result","tool_use_id":…,"content":outcome.content, "is_error": true (only when error), "mia_display": outcome.display}`.
  - `date_time::run(clock: &dyn Clock) -> ToolOutcome` — content `{"utc":"2024-01-01T12:00:00Z","timezone":"UTC"}`, display `{ name: "date_time", failed: false }`.

- [ ] **Step 1: Write the failing tests**
  - config: `assistant_configuration_defaults_when_section_absent` (deserialize a YAML without `assistant` → 30/60/8); `validate_fails_when_a_limit_is_zero`.
  - `run_tool_returns_error_outcome_when_tool_is_unknown` — name `"nope"` → `is_error`, content `unknown tool 'nope'`, display `failed: true`, `assert_statement_count(&log, 0)`.
  - `run_tool_returns_current_utc_time_when_date_time` — `fixed_clock(2024,1,1,12,0,0)` → content JSON `{"utc":"2024-01-01T12:00:00Z","timezone":"UTC"}`, display `ToolCallView { name: "date_time", failed: false, .. }`.
  - `result_block_includes_display_and_error_flag_when_outcome_failed` and `result_block_omits_is_error_when_outcome_succeeded`.
  - `definitions_offer_four_tools_with_object_schemas`.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p infrastructure -p services tools` → FAIL.

- [ ] **Step 3: Implement.** `mia_search`, `mia_details`, `tmdb_search` dispatch to modules that Tasks 6, 8 and 9 create; until then they return `failed(name, input, "not implemented")` — those tasks replace each arm.

- [ ] **Step 4: Run tests** — `cargo test --workspace` → PASS.

- [ ] **Step 5: Commit** — `git commit -m "Add assistant config, tool dispatcher and date_time tool"`

---

### Task 6: `mia_search` tool

**Files:**
- Create: `services/src/services/chat/tools/mia_search.rs`, `services/src/services/chat/tools/mia_search.md` (description, `include_str!`)
- Modify: `transpiler/src/lib.rs`, `transpiler/src/constructor.rs` (export field lists), `tools/mod.rs` (dispatch)
- Test: `mia_search.rs` and `constructor.rs` `mod tests`

**Interfaces:**
- Consumes: Task 4 `search_library`; Task 5 `ToolOutcome`, `failed`, config `assistant.mia_search_limit`.
- Produces:
  - `transpiler::FILTER_FIELDS: [&str; 17]` and `transpiler::SORT_TARGETS: [&str; 9]` (the names in `constructor::target` / `constructor::sort`).
  - `mia_search::DESCRIPTION: &str` (used by `definitions()`).
  - `mia_search::run(input: &Value, user: &CurrentUser, db: &DbConn) -> Result<ToolOutcome, SrvErr>`.
- Behaviour: `query` missing or not a string → `failed(…, "input must have a string 'query'")`. Calls `search_library(SearchQuery { query, genres: None, only_watched: false, min_stars: None, sort_by: SortTarget::Title }, PageReq { offset: None, limit: Some(limit) }, None, user, db)`. Transpile errors → `failed` with the messages joined by `\n`. Success: release dates loaded with one query per type present (`movies` / `series` rows, whose id is the media id); content `{"results":[{"id","type":"movie"|"series","title","release_year","overview","tmdb_id","stars"}]}` (absent values `null`); display `{ name: "mia_search", query, count: results.len(), failed: false }`.
- Description (`mia_search.md`) teaches: `title : filter : sort`, optional from the right, empty title allowed, ` \: ` escape; title is free text, unknown words in filter/sort are errors; every field of `FILTER_FIELDS` with its literal type and meaning (read each `*_target` in `constructor.rs`); operators, range form, `and` / `or`, parentheses, literal forms (`"string"`, `YYYY-MM-DD`, `true`/`false`, `null`, numbers); every `SORT_TARGETS` entry and `asc`/`ascending`/`desc`/`descending`; `has_tag` / `has_source_with_name` / `has_genre` take the person's exact names (case-insensitive) and an empty result may mean a wrong name; results are capped and include mia ids for `mia_details`.

- [ ] **Step 1: Write the failing tests**
  - constructor.rs: `target_accepts_every_filter_field` (for each of `FILTER_FIELDS`, `target(&field.to_string(), ComparisonOperator::Equal, Literal::Null)` is `Ok` or an `Err` whose message does not start with `Unknown target`); `sort_accepts_every_sort_target`.
  - `description_names_every_filter_field_and_sort_target` — `DESCRIPTION` contains each entry of both arrays.
  - `run_returns_hits_with_release_year_when_query_matches` — mock DB: transpiled query returns one movie row (`media_index_row`-style tuple via `media_model` with overview `"Space"`, tmdb_id 348, stars 4.5, title "Alien"), then `movies` row with `release_date` 1979-05-25 → content `{"results":[{"id":1,"type":"movie","title":"Alien","release_year":1979,"overview":"Space","tmdb_id":348,"stars":4.5}]}`, display count 1.
  - `run_returns_error_with_transpile_messages_when_query_is_invalid` — `" : foo = 1"` → `is_error`, content `Construction: Unknown target 'foo'`, display `{ query: " : foo = 1", failed: true }`.
  - `run_returns_error_when_query_is_missing`.
  - `run_returns_first_page_of_library_when_query_is_empty` (Review Focus) — `""` → not an error; results as queued.
  - `run_limits_results_to_configured_cap` — `assert_wrote`-free: assert the SELECT's `LIMIT` value is `30` via `test_support::db::log` values (the limit is bound as a value).

- [ ] **Step 2: Run them to see them fail** — `cargo test -p transpiler -p services mia_search` → FAIL.

- [ ] **Step 3: Implement** and wire the `mia_search` arm in `run_tool` and `definitions()`.

- [ ] **Step 4: Run tests** — `cargo test --workspace` → PASS.

- [ ] **Step 5: Commit** — `git commit -m "Add mia_search tool"`

---

### Task 7: Batched details loaders behind `movies::details` / `series::details`

**Files:**
- Create: `services/src/services/media_details.rs` (register in `services/src/services/mod.rs`)
- Modify: `services/src/services/movies.rs`, `services/src/services/series.rs`
- Test: `media_details.rs` `mod tests`; existing `movies::tests::details_*` / `series::tests::details_*`; `services/tests/postgres_media.rs`

**Interfaces:**
- Produces:
  - `media_details::MediaParts { pub title: String, pub alternative_titles: Vec<AlternativeTitle>, pub original_language: Option<Language>, pub genres: Vec<Genre>, pub tags: Vec<Tag>, pub sources: Vec<Source>, pub logs: Vec<Log>, pub on_watchlist: bool }` (`pub(crate)`) — exactly what `movies::details` / `series::details` assemble today from titles (primary → `title`, else `config().media.unset_title`), language, genres, tags, sources, logs (log `source` = its source's name) and watchlist.
  - `media_details::load_parts(media: &[media::Model], db: &DbConn) -> Result<HashMap<i32, MediaParts>, SrvErr>` — one query each for titles, languages, genres (via `media_genres`), tags (via `media_tags`), sources, logs and watchlist, all `is_in` over the given media: **7 queries whatever `media.len()`**, none for an empty slice. Each list ordered by id.
  - `movies::load_movies(media_ids: &[i32], db) -> Result<HashMap<i32, movies::Model>, SrvErr>` (1 query) and `movies::details_view(media: media::Model, movie: movies::Model, parts: MediaParts, locks: Vec<&'static str>, backdrop: Option<Image>, poster: Option<Image>) -> MovieDetails`, both `pub(crate)`.
  - `series::load_series(media_ids: &[i32], db) -> Result<HashMap<i32, series::Model>, SrvErr>` (1 query), `series::load_counts(series_ids: &[i32], db) -> Result<HashMap<i32, (i32, i32)>, SrvErr>` — `(number_of_seasons, number_of_episodes)` per series, one grouped query each (2 queries); `series::details_view(…)` like the movie one. All `pub(crate)`.
  - `movies::details` / `series::details`: same signatures and output, now built from the loaders for the single id (plus their own locks and `fetch_backdrop_and_poster`).

- [ ] **Step 1: Write the failing tests**
  - `load_parts_issues_seven_queries_when_given_three_media` — three media models; queue one result set per query (titles, languages, media_genres+genres, media_tags+tags, sources, logs, watchlist) covering all three; assert each media's `MediaParts` (e.g. media 2's `genres == [Genre "Horror"]`, media 3's `on_watchlist`, log source names resolved) and `assert_statement_count(&log, 7)`.
  - `load_parts_issues_no_queries_when_given_no_media`.
  - `load_counts_returns_seasons_and_episodes_per_series` — two series; `assert_statement_count(&log, 2)`.
  - Postgres (`services/tests/postgres_media.rs`, `#[ignore = "requires docker"]`): `load_parts_assigns_each_media_only_its_own_genres_tags_sources_and_logs` — seed two movies with different genres, tags, sources, logs, one on the watchlist (use `common::seed`), call `load_parts`, assert no cross-contamination; and `series_details_counts_seasons_and_episodes` if no existing Postgres test covers the counts. (`load_parts` is `pub(crate)`; expose what the integration test needs via `pub` on the module item with a doc comment, or test through `movies::details` for two media — implementer's call, but the junction queries must run against Postgres.)
  - Existing `details_*` tests in `movies.rs` / `series.rs`: re-queue mock results in the new query order; **keep every assertion unchanged**.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p services media_details` → FAIL.

- [ ] **Step 3: Implement** the loaders and rewrite `movies::details` / `series::details` on them.

- [ ] **Step 4: Run all suites**

Run: `cargo test --workspace` then `cargo test -p services --no-fail-fast --tests -- --ignored`
Expected: PASS (known-bug tests fail as before).

- [ ] **Step 5: Commit** — `git commit -m "Batch the details queries for movies and series"`

---

### Task 8: `mia_details` tool

**Files:**
- Create: `services/src/services/chat/tools/mia_details.rs`
- Modify: `tools/mod.rs` (dispatch, definition)
- Test: `mia_details.rs` `mod tests`

**Interfaces:**
- Consumes: Task 7 `load_parts`, `movies::load_movies`, `movies::details_view`, `series::load_series`, `series::load_counts`, `series::details_view`; config `assistant.mia_details_limit`.
- Produces: `mia_details::run(input: &Value, user: &CurrentUser, db: &DbConn) -> Result<ToolOutcome, SrvErr>`.
- Behaviour: `ids` must be an array of integers (else `failed(…, "input must have an integer array 'ids'")`); empty → `failed(…, "ids must not be empty")`; deduplicated keeping first-seen order; more than the cap (after dedup) → `failed(…, "at most {cap} ids per call")`. One query loads the user's media rows among the ids; then `load_parts`, `load_movies` (skipped when no movie), `load_series` + `load_counts` (skipped when no series): **at most 12 queries whatever the number of ids** (1 lookup + 7 parts + 1 movies + 1 series + 2 counts). Views built with `details_view(…, locks: vec![], backdrop: None, poster: None)`, serialized, `poster` / `backdrop` / `locks` keys removed, `"type": "movie"|"series"` added; media in request order. Content `{"media":[…],"not_found":[ids…]}` (missing or another user's, in request order). Display `{ name: "mia_details", titles: Some(titles of found media, in order), failed: false }`.

- [ ] **Step 1: Write the failing tests**
  - `run_returns_details_without_images_and_locks_in_fixed_query_count_when_movies_and_series_found` — ids `[5, 6, 9]` (movies 5, 6; series 9); queue lookup, the 7 part queries, movies, series, 2 counts → `media` in order 5, 6, 9 with `"type"` set and no `poster` / `backdrop` / `locks` keys; `assert_statement_count(&log, 12)`; display titles in order.
  - `run_skips_series_queries_when_only_movies_found` — `assert_statement_count(&log, 9)`.
  - `run_lists_missing_ids_as_not_found_when_lookup_misses` — lookup returns no rows for `[7]` → content `{"media":[],"not_found":[7]}`, `assert_statement_count(&log, 1)`, display titles `[]`.
  - `run_returns_error_when_ids_exceed_cap` — 4 distinct ids with test cap 3 → `is_error`, content `at most 3 ids per call`, `assert_statement_count(&log, 0)`.
  - `run_returns_error_when_ids_empty` (Review Focus) — content `ids must not be empty`.
  - `run_looks_up_each_id_once_when_ids_repeat` (Review Focus) — `[7, 7]` with the lookup returning nothing → `not_found == [7]`.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p services mia_details` → FAIL.

- [ ] **Step 3: Implement** and wire dispatch and definition (schema: `ids` array of integers, `minItems` 1).

- [ ] **Step 4: Run tests** — `cargo test --workspace` → PASS.

- [ ] **Step 5: Commit** — `git commit -m "Add mia_details tool"`

---

### Task 9: `tmdb_search` tool

**Files:**
- Create: `services/src/services/chat/tools/tmdb_search.rs`
- Modify: `tools/mod.rs`
- Test: `tmdb_search.rs` `mod tests`

**Interfaces:**
- Consumes: Task 4 `search_tmdb`, `TmdbHit`.
- Produces: `tmdb_search::run(input: &Value, user: &CurrentUser, db: &DbConn, tmdb: &dyn Tmdb) -> Result<ToolOutcome, SrvErr>`.
- Behaviour: `query` required string; `type` optional, `"movie"` | `"series"`, anything else → `failed(…, "type must be \"movie\" or \"series\"")` without calling TMDB. TMDB error → `failed(…, "TMDB search failed: {error}")`. One query finds the user's media whose `(tmdb_id, type)` match the hits (`media::Column::TmdbId.is_in(...)`, filtered by user, then matched by type in memory). Content `{"results":[{"tmdb_id","type","title","original_title","overview","year","original_language","tmdb_rating","mia_id"}]}` (`mia_id` null when not in the library). Display `{ name: "tmdb_search", query, media_type: type if given, count, failed: false }`.

- [ ] **Step 1: Write the failing tests**
  - `run_marks_hits_already_in_library_with_mia_id` — `MockTmdb` returns movie 348 and tv 1399; library lookup returns a movie media row (id 5, tmdb_id 348) → `results[0].mia_id == 5`, `results[1].mia_id == null`, display count 2.
  - `run_does_not_mark_hit_when_library_has_same_tmdb_id_with_other_type` — library row is a series with tmdb_id 348 → movie 348's `mia_id` null.
  - `run_keeps_only_requested_type_when_type_given` — `type: "series"` → only the tv hit; display `media_type: Some("series")`.
  - `run_returns_error_without_calling_tmdb_when_type_is_invalid` (Review Focus) — `type: "tv"` → `is_error`; `MockTmdb` without expectations proves no call.
  - `run_returns_error_when_tmdb_fails` — `search_multi` returns `test_support::tmdb::error("down")` → `is_error`, display `failed: true`.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p services tmdb_search` → FAIL.

- [ ] **Step 3: Implement** and wire dispatch and definition.

- [ ] **Step 4: Run tests** — `cargo test --workspace` → PASS.

- [ ] **Step 5: Commit** — `git commit -m "Add tmdb_search tool"`

---

### Task 10: Tool rounds in the Exchange loop

**Files:**
- Modify: `services/src/services/chat/exchange.rs`, `services/src/services/chat/failure.rs`, `services/src/services/chat/history.rs` (send `tools::definitions()`), `services/src/services/chat/system_prompt.md`
- Modify: `services/src/jobs.rs` (`JobDeps.tmdb`), `services/src/test_support/jobs.rs` (`job_deps` gets `tmdb: Arc::new(MockTmdb::new())`), `services/src/test_support/db.rs` (round helper), `api/src/lib.rs` (pass `tmdb` into `JobDeps`), `services/tests/postgres_chat.rs` (its own `JobDeps` gets `tmdb`)
- Modify: `CLAUDE.md` (`JobDeps` holds `tmdb`), `CONTEXT.md` (add **Tool round**: one LLM call that requested tools plus the Tool result Message answering it)
- Test: `exchange.rs`, `history.rs`, `failure.rs` `mod tests`

**Interfaces:**
- Consumes: Tasks 2, 3, 5 (`Accumulator::tool_uses` / `tool_use_cut`, `DISPLAY_FIELD`, `display_blocks`, `ExchangeEvent::ToolCall`, `tools::run_tool` / `failed` / `result_block`, config `assistant.max_tool_rounds`).
- Produces:
  - `run_exchange(exchange_id: i32, db: &DbConn, llm: &dyn Llm, registry: &ExchangeRegistry, tmdb: &dyn Tmdb, clock: &dyn Clock) -> Result<(), SrvErr>`.
  - `ExchangeFailure::OutputTruncated`, `ExchangeFailure::ToolRoundLimit`.
  - `JobDeps { …, tmdb: Arc<dyn Tmdb>, … }`.
  - `test_support::db::with_tool_round(db: MockDatabase, assistant_message_id: i32, tool_message_id: i32, next_llm_call_id: i32, exchange_id: i32) -> MockDatabase` — queues, for a round that runs only `date_time`: the assistant Message insert, the closed LLM call, the user row (`user_model(1)`, loaded once on the first round), the tool Message insert and the next LLM call insert, plus the exec results those writes consume. (Use it repeatedly for multi-round tests; the user row only on the first.)
- Loop rules (spec, *The Exchange loop*): per call, decide in this order — stream failed/unfinished → as today; `Refusal` → `Refused`; `tool_use_cut()` → `OutputTruncated`; any tool_use → tool round (warn if stop reason ≠ `ToolUse`); otherwise by stop reason as today, with `ToolUse` judged like `EndTurn` and `EmptyResponse` when the final call has no text.
- Tool round: close the call's `llm_calls` row (no error); load the user once (`users::Entity::find_by_id(conversation.user_id)` → `CurrentUser::from`); if this is the round after a refused one → end `ToolRoundLimit` (no results stored); else if rounds already done == `max_tool_rounds` → every tool_use gets `failed(name, input, "tool limit reached; answer with what you have")` and the round is marked refused; else each tool_use runs via `run_tool` (input `None` → `failed(…, "input is not a JSON object")`). Insert one `tool` Message (`llm_call_id` NULL) with `result_block`s in tool_use order; insert failure + Exchange gone → `Deleted`. Publish one `ToolCall` per result, numbered from `display_blocks(current Exchange's messages before the tool Message).len()`. Keep the Exchange's messages in memory (append the assistant and tool Messages) for the next request and the next `Accumulator::new(first_block)`.
- `history::build_request` sets `tools: tools::definitions()`.
- System prompt: add one short paragraph per spec *System prompt*.

- [ ] **Step 1: Write the failing tests** (in `exchange.rs`; LLM via `with_answers`, tools via `date_time` unless stated):
  - `run_exchange_runs_tool_round_then_completes_when_model_answers_after_tool` — answers: `tool_use_answer([("tu_1","date_time",{})], ToolUse)`, `text_answer(["It is noon"], EndTurn)` → events `[ToolCall { block: 0, call: { name: "date_time", failed: false } }, TextDelta { block: 1, "It is noon" }, Completed]`; `assert_wrote` the tool Message insert holds `tool_result` for `tu_1` with `mia_display`; Exchange `Completed`.
  - `run_exchange_answers_all_calls_in_one_tool_message_when_round_has_two_calls` — two tool_uses → one `INSERT INTO "messages"` with role tool holding two results; events `ToolCall{0}`, `ToolCall{1}`.
  - `run_exchange_numbers_tool_blocks_after_text_when_message_has_text_and_tool_use` — `text_then_tool_use_answer(["Let me check"], [date_time], ToolUse)` then text → `TextDelta{0}`, `ToolCall{1}`, `TextDelta{2}`.
  - `run_exchange_runs_tools_when_tool_use_arrives_with_end_turn` — first answer's stop reason `EndTurn` → still a tool round.
  - `run_exchange_completes_when_stop_reason_is_tool_use_without_tool_use_blocks` — `text_answer(["Hi"], ToolUse)` → `Completed`.
  - `run_exchange_fails_with_output_truncated_when_max_tokens_cuts_tool_use` — tool_use without `BlockStop`, stop `MaxTokens` → `failed("OutputTruncated", …)`, no tool Message.
  - `run_exchange_refuses_tools_at_round_limit_then_fails_with_tool_round_limit` — test config `max_tool_rounds: 2`; four answers all `date_time` tool_use → rounds 1–2 run, round 3's result content is `tool limit reached; answer with what you have` with `is_error: true` and display `failed: true`, the fourth call ends the Exchange `ToolRoundLimit` with no further tool Message.
  - `run_exchange_completes_when_model_answers_after_refused_round`.
  - `run_exchange_fails_with_empty_response_when_final_call_has_no_text_after_tool_round`.
  - `run_exchange_answers_with_error_result_when_tool_input_is_not_an_object`.
  - `run_exchange_stops_quietly_when_conversation_deleted_during_tool_round` (Review Focus) — tool Message insert errors and the existence check finds no Exchange → no terminal event, `llm_calls` closed with `{"message":"conversation deleted"}`. *(Superseded by the Task 10 ruling: the call stays closed with `tool_use` and no error; it is not re-closed.)*
  - `run_exchange_sends_current_exchange_thinking_and_tool_blocks_in_next_request` — capture the second `stream_message` request (`withf`/`returning` storing it) → it contains the assistant Message with its thinking and tool_use, then the tool Message without `mia_display`, and `tools` holds the four definitions.
  - `failure.rs`: keys render as `OutputTruncated` / `ToolRoundLimit`.
  - Existing `run_exchange` tests: update calls for the new `tmdb` parameter; `run_exchange_..._tool_use` expectations that relied on `ToolUse → Internal` change per the new rule.

- [ ] **Step 2: Run them to see them fail** — `cargo test -p services exchange` → FAIL.

- [ ] **Step 3: Implement** the loop, failures, `JobDeps.tmdb` and wiring (`api/src/lib.rs`, `test_support::jobs::job_deps`, `postgres_chat.rs`), system prompt, `CLAUDE.md`, `CONTEXT.md`.

- [ ] **Step 4: Run all suites**

Run: `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo test -p services --no-fail-fast --tests -- --ignored`
Expected: PASS (known-bug tests fail as before); no new clippy warnings.

- [ ] **Step 5: Manual check** — `cargo run -- serve` with a real `config.yaml`, create a Conversation, post "What sci-fi have I rated 4 stars or more?" and confirm the SSE stream shows `tool_call` events then text, and `GET /conversations/{id}` shows the same blocks.

- [ ] **Step 6: Commit** — `git commit -m "Run tool rounds in Exchanges with a round limit"`
