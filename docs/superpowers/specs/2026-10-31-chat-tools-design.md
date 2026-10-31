# Chat tools — design

Date: 2026-10-31
Builds on: `2026-10-29-llm-chat-endpoints-design.md` (plain chat) and `2026-10-31-reasoning-design.md` (thinking), both on master as of `49f246b`.

## Intent

The assistant can only talk today; it knows nothing about the person's library and cannot look anything up. This increment gives it tools: search the person's library, read the full details of media in it, search TMDB, and know the current date. The person sees each tool call in the conversation, to the extent it tells them something.

The plain-chat spec required that tools slot in "as a new branch, not a rewrite". The `ToolUse` arm of the Exchange loop, the `tool` Role, the Tool result Message and the Exchange-wide block numbering all exist for this. Use them; do not reshape the Exchange flow, the SSE model or the display rule beyond what is described here.

Domain terms (Conversation, Exchange, Message, Tool result, LLM call, Partial message, …) are in `CONTEXT.md`. Use them exactly.

## Goals

- Four tools offered on every LLM call of an Exchange: `mia_search`, `mia_details`, `tmdb_search`, `date_time`.
- The Exchange loop runs tool rounds until the model gives a normal answer, bounded by a configurable round limit.
- Every tool call is visible to the person as its own display block, live and on reload, showing only what tells them something.
- Client state = DB state still holds: a tool block reaches the client only after what it is derived from is committed.

## Non-goals

- Widgets / display tools (media cards, maps). They remain a later increment.
- Any TMDB search endpoint other than `search/multi`; TMDB detail lookups as tools.
- Season and episode data in `mia_details`.
- Tools that change data (adding media, logging a watch, editing the watchlist).
- The person's timezone. `date_time` answers in UTC. **Note for later:** look into geolocation (or another source of the person's location) for proper timezone handling.
- Trimming tool results out of later Exchanges' history. Large results stay in history; long conversations are left to future compaction (*user's earlier decision*).
- Retry / Regenerate.
- Running a round's tool calls in parallel.

## Anti-goals

- Changing the public API or behaviour of `services::media::search` (or of `movies::details` / `series::details`).
- Tools calling `transpiler::transpile` or `Tmdb::search_multi` directly. Reuse what can be shared with `media::search` instead (see *Shared code*).
- Relying on any one model's tool behaviour: `tool_choice`, parallel-call support, or how a model reacts to missing tool definitions. Models are open models via OpenRouter and will change (see the endpoints spec's constraints). Only plain `tools` definitions plus `tool_use` / `tool_result` blocks are used.
- Showing the person ids, raw JSON, or result lists in tool blocks.
- Hand-rolling any part of the OpenRouter client. openrouter-rs 0.22 already supports `tools` on Anthropic messages requests and `tool_use` / `tool_result` content parts.

## Configuration

New optional section in `config.yaml` (add to `infrastructure::config::Configuration`, `config.yaml.example` and the config block in `CLAUDE.md`). Optional with these defaults, so existing config files keep working (*my call*):

```yaml
assistant:
  mia_search_limit: 30     # fixed cap on mia_search results
  mia_details_limit: 60    # fixed cap on ids per mia_details call
  max_tool_rounds: 8       # see "Round limit"
```

The caps are **fixed**, not parameters the model can pass. Validate that each is ≥ 1 at load, like `openrouter.validate()`.

## The tools

Tool names, input schemas (JSON Schema) and descriptions are defined once in `services` and sent on every LLM call of an Exchange. Descriptions carry the usage detail; the system prompt only gets a short paragraph on when to use which tool (*my call*). Tools run as the Conversation's owner: `user_id` from the Conversation, never anything from the model.

### `mia_search`

- **Input:** `{ "query": string }`, required.
- **Query language:** the full language `transpiler` parses. The description must teach the model:
  - A query has up to three parts separated by ` : ` (space-colon-space): `title : filter : sort`. Parts are optional from the right; an empty title is allowed (e.g. ` : stars >= 4 : date_added desc`, check the exact form the parser accepts). A literal ` : ` inside the title is escaped as ` \: `.
  - The title part is free text matched against titles. Words in the filter and sort parts are not titles: an unrecognised identifier there is an error.
  - Filter fields: `stars`, `watched`, `times_watched`, `type`, `has_source`, `number_of_sources`, `has_source_with_name`, `has_tag`, `has_genre`, `language`, `id`, `tmdb_id`, `release_date`, `date_added`, `on_watchlist`, `rated`, `tmdb_rating`, each with its literal type (int, float, string in double quotes, date `YYYY-MM-DD`, `true`/`false`, `null`). Operators `= != < <= > >=`, the range form `low < field <= high`, `and`, `or`, parentheses. `type` takes `"movie"` or `"series"`.
  - Sort targets: `stars`, `watched`, `times_watched`, `type`, `has_source`, `number_of_sources`, `release_date`, `date_added`, `tmdb_rating`, with optional (asc/desc) direction.
  - `has_tag`, `has_source_with_name` and `has_genre` match the person's own names (case-insensitive, otherwise exact), which the model cannot know; a wrong guess returns zero hits, not an error. This is **accepted** for now: no name lists in the description, no extra tool, no special error (*user's decision*). The description should say these take exact names and that an empty result may mean a wrong name.
  - The builder must derive the exact grammar (direction keywords, which fields accept which literals, what `watched`/`rated` compare against) from `transpiler/src/parser.rs` and `constructor.rs`, not from this list, and keep the description in sync with them. A test should guard that every field and sort target the constructor accepts is named in the description (*my call*).
- **Scope:** the current user's library only. No TMDB fallback; that is `tmdb_search`'s job.
- **Limit:** `assistant.mia_search_limit` results, first page only.
- **Result per hit:** mia `id`, `type` (`movie` | `series`), primary title, release year (movie `release_date` / series `first_air_date`), overview, `tmdb_id`, the user's `stars`. No images.
- **Invalid query:** the tool result is an error (`is_error: true`) listing every lexing, parsing and construction message, so the model can correct the query within the same Exchange. (`media::search` today collapses these to `query_valid: false`; the shared helper must expose the messages without changing `media::search`.)

### `mia_details`

- **Input:** `{ "ids": [int] }`: mia media ids (movies and series may be mixed), 1 to `assistant.mia_details_limit` entries. More than the cap → error result saying so; the model can split the call.
- **Result per found id:** everything `movies::details` / `series::details` return except images (poster, backdrop) and `locks`: type, title, alternative titles, release date or first air date, runtime / number of episodes and seasons, status, series type, overview, `tmdb_id`, TMDB rating, the user's stars, on-watchlist, original language, genres, tags, sources, logs (date, stars, completed, comment, and the log's source). **Series level only**: no seasons or episodes.
- **Not found / another user's:** listed in the result as not found; the call still succeeds for the rest. Another user's media is indistinguishable from a missing one.
- **Batched, never n+1** (*user's decision*: minimizing database calls is a project goal). Do not call `movies::details` / `series::details` per id. Refactor them into batched loaders that take many media at once and issue a fixed number of queries (`is_in` over all ids) regardless of how many ids are asked for; `movies::details` and `series::details` become thin single-id wrappers over those loaders, keeping their public signatures and output. `mia_details` uses the same loaders, skipping what it does not return (images, locks). Its query count must not grow with the number of ids, pinned by a test.

### `tmdb_search`

- **Input:** `{ "query": string, "type"?: "movie" | "series" }`.
- **Backend:** TMDB `search/multi` through the existing `Tmdb` trait, one page (up to 20 results), adult excluded, as today.
- **Filtering:** as the mia search endpoints do: keep movie and TV results, drop people and collections; with `type` given, keep only that kind. Reuse the filtering logic shared with `media::search` rather than calling `search_multi` from the tool directly.
- **Already in the library:** hits are **kept** (unlike `media::search`, which drops them) and marked with the person's mia `id`, so the model can go straight to `mia_details`. Matching is by `tmdb_id` and type, as `media::search` matches.
- **Result per hit:** `tmdb_id`, `type`, title, original title, overview, release / first-air year, original language, TMDB rating, mia `id` if in the library.
- **TMDB failure** (network, non-2xx): error result for the model; the Exchange goes on.

### `date_time`

- **Input:** none (`{}`).
- **Result:** the current date and time from `Clock`, ISO 8601 in UTC, labelled as UTC so the model knows.
- Timezone handling is a non-goal; see the note under *Non-goals*.

### Tool errors

Anything that goes wrong *inside* a tool (unknown tool name, input that does not match the schema or is not valid JSON, an invalid query, too many ids, TMDB failing) becomes a Tool result with `is_error: true` and a short message the model can act on. It never fails the Exchange. Only our own infrastructure failing (DB errors while running a tool or writing its result) fails the Exchange, as `Internal`, like any other DB failure in the run.

## Shared code

`media::search` keeps its signature and behaviour exactly. Extract what the tools need into internal (`pub(crate)` or private-module) helpers that `media::search` itself then calls (*user's decision: reuse, don't fork*):

- the library query: transpile with the page limit, run it, load poster sizes; returning the transpile errors as messages, which `media::search` keeps mapping to `query_valid: false`;
- the TMDB multi filtering: movie/TV only, optional type, matching against the library by `tmdb_id` and type. `media::search` keeps dropping matched hits and applying its `external_limit`; the tool keeps and marks them.

The existing Postgres search tests (`services/tests/postgres_search.rs`) and `media::search` unit tests must pass unchanged; they are the guard that the refactor did not change behaviour.

## The Exchange loop

`Job::RunExchange` becomes a loop over LLM calls (the plain-chat spec already asked for "a loop over LLM calls with a match on a typed stop-reason enum"):

1. Build the request from history, including the current Exchange's Messages so far, with the tool definitions.
2. Make the LLM call exactly as today: `llm_calls` row first, stream, write-first-forward-second, accumulator.
3. Decide by **content first, stop reason second** (*my call*). Providers behind OpenRouter do not agree on stop reasons when tools are involved (some report `stop`/`end_turn` alongside tool calls), and a completed Exchange holding an unanswered `tool_use` would poison every later request of the Conversation, since history replays it verbatim and the Anthropic format rejects an unanswered `tool_use`. So:
   - A `tool_use` block was cut off (not stopped, or its input JSON unparseable) by `MaxTokens` → the Exchange fails with **`OutputTruncated`** (reserved by the endpoints spec, added now). A cut-off `tool_use` from a stream that failed or ended unfinished fails as today.
   - The call ended normally (any stop reason except `Refusal`: `ToolUse`, `EndTurn`, `StopSequence`, `MaxTokens` after the tool_use finished, unknown, missing) **and** the assistant Message holds at least one finished `tool_use` → a tool round: run every `tool_use` block, in order, one after another. Store **one** `tool` Message holding a `tool_result` for each (`CONTEXT.md`: "An assistant Message that requests tools is followed by exactly one Tool result message answering all its calls"). Publish the tool blocks (below). Next round. Log a warning when the stop reason was not `ToolUse`.
   - No `tool_use` blocks → as today, by stop reason: `EndTurn` / `StopSequence` / `MaxTokens` / unknown / missing complete, `Refusal` fails. `ToolUse` with no `tool_use` block is no longer `Internal`: it is judged like `EndTurn` (completes if text arrived, `EmptyResponse` otherwise).
   - `EmptyResponse` applies to the final call, i.e. a final call that produced no text fails the Exchange even if earlier rounds produced text (*my call*; it means the model never answered).
   - Invariant: a **completed** Exchange never contains a `tool_use` without a matching `tool_result`.
4. Each round is its own LLM call with its own `llm_calls` row and its own assistant Message (*existing rule*: one LLM call → at most one assistant Message).

### Accumulator

Gains the `tool_use` arm the endpoints spec left room for: `BlockKind::ToolUse { id, name }` (new in `integrations`), `input_json_delta` concatenated and parsed at block stop. `integrations::openrouter` needs: `LlmRequest.tools`, translation of `tool_use` block starts and `input_json_delta`, and passing `tool_use` / `tool_result` content through `translate_message`.

Stored `tool_use` blocks must replay through the Anthropic format: `input` must be a JSON object. If the model's input is not valid JSON or not an object, store `{}` (or the closest valid form; builder's call) and answer that call with an error result (*my call*).

### History

- The current Exchange's `tool` Messages are sent with role user (already mapped this way in `history::build_request`).
- Thinking in the **current** Exchange is kept across its tool rounds; models that reason need it. Earlier Exchanges' thinking stays stripped, as today.
- `redacted_thinking` blocks are now **stored** verbatim (the reasoning spec ignored them and deferred the question to this increment), so they are sent back unchanged within the current Exchange's tool rounds, as Anthropic-format providers expect. Not displayed, not counted as text; stripped from earlier Exchanges as today (*my call*).
- Earlier completed Exchanges' `tool_use` / `tool_result` blocks stay in history verbatim.
- Failed Exchanges are still left out entirely, tool rounds included.

### Round limit

`assistant.max_tool_rounds` (default 8) counts LLM calls that ended in `ToolUse` within one Exchange (*user's choice: option 3*):

- Up to the limit, tools run normally.
- When a call ends in `ToolUse` and the limit is already reached, every `tool_use` in it gets an error result: "tool limit reached; answer with what you have". The tools are not run. The model gets one more call.
- If that call also ends in `ToolUse`, the Exchange fails with the new key **`ToolRoundLimit`**. That last call's `tool_use` blocks get no results.
- Tool definitions stay in every request; the limit never relies on `tool_choice` or on removing tools.

### Interrupted and deleted

Unchanged rules: an Exchange interrupted mid-round is marked `Interrupted` at startup; a Conversation deleted mid-round stops the run quietly at the next write. Tool execution itself writes nothing but the Tool result Message, so the existing "Exchange row gone → Deleted" handling covers it.

## What the person sees

### Tool blocks

Each tool call becomes a display block of its own, `{ "type": "tool", ... }`, alongside `text` and `thinking`:

| Tool | Shown |
|---|---|
| `mia_search` | the query; the result count |
| `tmdb_search` | the query; the type, if given; the result count |
| `mia_details` | the titles of the media looked up (not the ids); ids not found are left out |
| `date_time` | just the tool (*my call*: every call visible, nothing else to show) |

A call whose result was an error (bad query, TMDB down, round limit) still gets its block, marked failed, with no further detail (*my call*). The model usually tries again; the person sees both attempts. Exact DTO fields are the builder's call; the content above is not.

### When and how it is sent

- A tool block is sent **after the tool has run** and its Tool result Message is committed, never at `tool_use` time.
- New SSE event **`tool_call`** carrying `{ "block": n, ... }` with the whole block. It is not streamed in parts. The SSE event set is now `text_delta`, `thinking_delta`, `tool_call`, `exchange_completed`, `exchange_failed`.
- Rejoining replays tool blocks as `tool_call` events, in block order with the rest (`ExchangeEvent::replay`).

### Derivation and numbering

- Live, rejoin and reload use **one** derivation rule (`chat::display`), as today. It now reads the assistant Messages **and** the Tool result Messages: a tool block comes from a `tool_use` block plus its matching `tool_result` (by `tool_use_id`).
- Everything shown must be derivable from stored Messages alone (*client state = DB state*). The titles for `mia_details` and the result counts must therefore be recoverable from what the Tool result Message stores; looking them up again at display time is not allowed, since a reload must show what the model saw. How to store them so they are derivable is the builder's call, with one constraint: **whatever is sent to the model must stay valid Anthropic content**. Either the display reads the model-facing result content, or a mia-only field is stored and stripped in `history::build_request` before sending. Do not rely on OpenRouter ignoring unknown fields.
- Numbering stays Exchange-wide, in message order. **Within an assistant Message, its tool blocks come after all its text and thinking blocks**, in `tool_use` order (*my call*). That is also the order things happen: the Message streams, then its tools run. So after one round an Exchange might read: thinking 0, tool 1, tool 2, text 3 (the text coming from the next call).
- A `tool_use` with no `tool_result` yet (an in-progress Exchange while its tools run, between the assistant Message's commit and the Tool result Message's commit) or never (a failed Exchange: Partial message, `ToolRoundLimit`, interruption) is **not shown**. Because tool blocks sit at the end of their Message, hiding them shifts no other number: a reload mid-round shows exactly what the live stream has sent. No "pending" state exists.
- The accumulator numbers only text and thinking, as today. The next call's first block number is the count of display blocks already derived for the Exchange, which includes the tool blocks of finished rounds (as `run_exchange` computes it today).

## Failures

Two new keys on `ExchangeFailure`:

| Key | When |
|---|---|
| `OutputTruncated` | `MaxTokens` cut through a `tool_use` block |
| `ToolRoundLimit` | the model asked for tools again after its calls were refused at the round limit |

Everything else as in the endpoints spec.

## Dependencies

- `JobDeps` gains `tmdb: Arc<dyn Tmdb>` and passes it to the run. `run_exchange` takes `tmdb` (parameter order per `CLAUDE.md`: `db, llm, registry, tmdb, …, clock`).
- `date_time` uses the existing `Clock`.
- No new traits. Tool execution is ordinary `services` code using the injected dependencies.

## System prompt

Add a short paragraph to `chat/system_prompt.md`: the assistant can search the person's library, read details of their media, search TMDB for things not in the library, and check the date; prefer the library for questions about what the person owns, watched or rated. The tool descriptions carry the details. Keep it model-neutral.

## Testing

Follow `CLAUDE.md`'s testing rules exactly. After changes: `cargo test --workspace`, `cargo clippy --workspace --all-targets` with no new warnings, the Postgres suite (the search refactor touches SQL paths), and the OpenRouter contract test.

**Unit tests** (main kind; mock DB, `MockLlm`, `MockTmdb`, `RecordingJobs`, `FixedClock`):
- `integrations`: translation of `tool_use` block starts, `input_json_delta`, `tools` in the request, `tool_use` / `tool_result` content passing through `translate_message`.
- Accumulator: tool_use blocks built from deltas, input parsed at block stop, invalid input; `redacted_thinking` stored.
- Each tool: input validation, caps, result shape, error results (invalid query with its messages, too many ids, TMDB error), library marking in `tmdb_search`, type filter, not-found ids in `mia_details`, `date_time` from `FixedClock`.
- The loop: a tool round then an answer; several calls in one round answered in one `tool` Message; a tool round when finished `tool_use` blocks arrive with `end_turn` or another non-`tool_use` stop reason; `tool_use` stop reason without `tool_use` blocks; `OutputTruncated`; the round limit (refusal results at the limit, then `ToolRoundLimit`); thinking kept within the Exchange; `EmptyResponse` on the final call.
- Display: tool block derivation and numbering shared by stream and view; `tool_call` replay on rejoin.
- A guard that the `mia_search` description names every filter field and sort target the constructor accepts.
- `test_support` gains builders for tool-use streams in `test_support::llm`, never local to a test.

**Postgres tests:** the batched details loaders (junction-table queries over many media, each media getting only its own genres / tags / sources / logs) get a real-Postgres test; the mock DB cannot prove that assignment. The existing `postgres_search.rs` must pass unchanged.

**Live OpenRouter contract test:** extend `integrations/tests/openrouter_contract.rs` with one test: a request offering a trivial tool and a prompt that calls for it, on `openrouter/free`, asserting a `tool_use` block arrives and its input parses. `openrouter/free` may route to a model without tool support; if that makes the test unreliable, the builder may pin a specific free tool-capable model for this test only and say so.

## Docs to update

- `CONTEXT.md`: add **Tool call** / **Tool round** if the builder needs the terms; keep the existing definitions.
- `CLAUDE.md`: the `assistant` config block; `JobDeps` now holds `tmdb`.

## Left to the builder

Module layout (e.g. `services/src/services/chat/tools/`), DTO field names for the `tool` block and the `tool_call` event, the JSON shape of tool results sent to the model, how display data is stored in or derived from Tool result Messages (within the constraint above), how the shared search helpers and the batched details loaders are split.

## Calls I made (check these)

- `assistant` config section is optional with defaults 30 / 60 / 8, and validated ≥ 1.
- Tool usage detail lives in tool descriptions; the system prompt gets one short paragraph.
- `mia_details` over the cap → error result, not truncation.
- Another user's media in `mia_details` is reported as not found.
- Malformed tool input is stored as a valid object and answered with an error result.
- `EmptyResponse` judged on the final call of the Exchange.
- Error tool calls still get a (failed) block; `date_time` gets a bare block.
- A test guards that the `mia_search` description covers every constructor field and sort target.
- The contract test may pin a free tool-capable model if `openrouter/free` proves unreliable for tools.
- Tool calls within a round run sequentially.
- Content decides a tool round, not the stop reason: finished `tool_use` blocks always get run and answered, so a completed Exchange never holds an unanswered `tool_use`.
- Tool blocks sit after their Message's text and thinking blocks; a `tool_use` without a result is hidden (no pending state).
- `redacted_thinking` is now stored and replayed within the current Exchange.
