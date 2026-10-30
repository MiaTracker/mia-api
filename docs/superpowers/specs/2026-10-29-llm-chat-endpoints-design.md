# LLM chat endpoints — design

Date: 2026-10-29
Branch: `llm-chat-schema` (schema landed in `5f1fc39`)

## Intent

Mia gets a plain LLM chat assistant: a user opens a Conversation, sends a message, and watches the answer stream in. This is the first increment. Tools, display-tool Widgets (media cards, maps), Retry/Regenerate/edit and LLM-generated titles come later, and they must slide into what is built here without reshaping it. Every design choice below is judged against that.

Domain terms (Conversation, Exchange, Message, Partial message, LLM call, Retry, …) are defined in `CONTEXT.md`. Use them exactly.

## Goals

- Create, list, read and delete Conversations.
- Send a user message and stream the model's answer over SSE.
- What the client is shown always equals what is stored: client state = DB state.
- Every request ever sent to OpenRouter is recorded in `llm_calls`, whatever its outcome.
- Every failure is visible to the person, with a typed reason.

## Non-goals (this increment)

- Tools, Tool results, Widgets.
- Retry, Regenerate, edit/Truncate.
- Conversation titles (`title` stays null; no rename endpoint).
- Extended thinking.
- Stopping an Exchange mid-stream (the registry leaves room for it).
- Per-conversation or per-request model choice.
- Trimming or summarising long Conversations.
- Telling the model the current date (will be a tool later).

## Anti-goals

- A design where adding tools means rewriting the Exchange flow, the SSE event set or the client's rendering. Tools must be a new branch, not a rewrite.
- The client ever showing text that is not in the database.
- Hand-rolling an OpenRouter client.
- Deleting from `llm_calls`, ever.

## Constraints

- Layering stays strict: `api` → `services` → `integrations`/`entities`/`views`. All SeaORM queries live in `services`.
- Provider: OpenRouter through its Anthropic-compatible Messages endpoint, via the crate **`openrouter-rs` (0.22)** — its `messages()` / `stream_messages` API and `Anthropic*` types. Do not write our own HTTP/SSE client for OpenRouter.
- Message content is stored as verbatim Anthropic content blocks in `messages.content` (JSONB), as the schema already decided.
- **Models will be open models, not Anthropic's**, reached through OpenRouter's Anthropic-compatible endpoint, and the model will change over time. Nothing may depend on a particular model's or upstream provider's behaviour or error wording. Classify on what OpenRouter guarantees (HTTP status, and its canonical `error_type` where we receive it), never on message text.
- The testing rules in `CLAUDE.md` (merged in `5981f42`) apply in full: outside-world access only through traits passed as `&dyn` parameters, no `Utc::now`/`tokio::spawn`/`std::fs` in `services/src`, background work through `Jobs`, test naming and `#[ignore]` reasons, fakes in `services::test_support`. See *Dependencies* and *Testing* below.
- Schema from `m20261029_000001_llm_chat` is the starting point; notable constraints: one `in_progress` Exchange per Conversation (partial unique index); `messages.llm_call_id` is set iff role is `assistant`; `llm_calls` FKs are `SET NULL` on delete.

### Schema change: error columns become JSONB

`exchanges.error` and `llm_calls.error` change from `string` to `json_binary` (nullable), since both hold structured JSON. The chat migration exists only on this unmerged branch, so **edit `m20261029_000001_llm_chat` in place** rather than adding a migration, regenerate the entities (`sea-orm-cli generate entity …`), and re-apply locally with `cargo run -- migrate down 1` then `cargo run -- migrate up` (always pass the count — plain `down` rolls back everything). `messages` has no error column and gets none.

## Dependencies

### The model provider behind a trait

A sixth outside-world trait, alongside `Tmdb`, `ImageStore`, `Jobs`, `Clock` and `Random`:

- **`integrations::openrouter::Llm`** — `async fn stream_message(&self, request: LlmRequest) -> Result<LlmEventStream, Error>`, where `LlmEventStream` is a `BoxStream<'static, Result<LlmEvent, Error>>`.
- **`LlmEvent` and `LlmRequest` are our own types**, defined in `integrations` (*my call*). `services` never sees openrouter-rs types. Reasons: openrouter-rs deltas are untyped `serde_json::Value` and several of its types are `#[non_exhaustive]`, which makes them awkward to build in tests; and the TMDB client already sets the pattern of `integrations` handing typed views to `services`. `LlmEvent` mirrors the Anthropic stream (message start with id and input/cache usage; block start with index and kind; text delta; block stop; message delta with stop reason and output tokens; message stop; ping can be dropped in the client). Error responses and in-stream `error` events surface as an `Error` carrying status and raw provider JSON, so `services` can classify them and fill `llm_calls.error`. Content in `LlmRequest` stays as Anthropic content-block JSON, so stored `messages.content` passes through untouched.
- **Real:** `OpenRouterClient` wrapping openrouter-rs, constructed from config (key, model, max_tokens) in `api::launch`, held in `AppState` as `Arc<dyn Llm>`. It does the untyped-delta parsing.
- **Fake:** `MockLlm` (mockall, like `MockTmdb`) in `services::test_support`, plus builders in `test_support` (e.g. an event-stream builder for a text answer, a stream that errors after N deltas, error builders for an HTTP status and for a mid-stream canonical `error_type`). Add them to `test_support`, never locally in a test.
- Update the trait table and rules in `CLAUDE.md` to list `Llm` (six traits) and the new `Job` variant.

### The Exchange runs as a `Job`

`services/src` may not call `tokio::spawn`, so the background task from the lifecycle below is a new variant **`Job::RunExchange { exchange_id }`**, submitted through `&dyn Jobs`. `JobDeps` gains what the handler needs: `llm`, `clock` and the Exchange registry. In tests, `RecordingJobs` records the job and the test runs the handler directly; with real Postgres, `InlineJobs` runs it on the spot.

The handler must never leave its Exchange `in_progress` on an error return: any `Err` ends the Exchange as failed with `Internal`. (A panic is caught and logged by `TokioJobs`; the Exchange then stays `in_progress` until the next startup marks it `Interrupted`. Acceptable.)

### Time

Every timestamp (`created_at`, `updated_at`, `completed_at`) comes from `Clock`. The ~60 s idle timeout is a `tokio::time::timeout`, not wall-clock arithmetic, so it can be tested with Tokio's paused time.

### Parameter order

Services functions follow `CLAUDE.md`: own parameters ending with `db`, then the dependencies. `llm` goes right after `tmdb`'s position and the registry right after `llm`: `db, llm, registry, tmdb, store, jobs, clock, random`, each only where needed (*my call*).

## Configuration

New required section in `config.yaml` (add to `infrastructure::config::Configuration` and to the config block in `CLAUDE.md`):

```yaml
openrouter:
  api_key: <OpenRouter key>
  model: <OpenRouter model id, an open model>
  max_tokens: 16384     # optional, default 16384
```

One model for everyone. Each LLM call row records the model it used.

## System prompt

A markdown file inside the `services` crate, compiled in with `include_str!`. Versioned with the code because once tools exist the prompt describes them. Content: short, says the assistant lives in Mia, a personal movie and TV tracker. No date, nothing personal about the user.

## Endpoints

All under the user router (`routes::build()`), documented in OpenAPI like the rest.

| Method & path | Behaviour |
|---|---|
| `POST /conversations` | Creates an empty Conversation for the current user. JSON: the conversation (id, title, timestamps). 201. |
| `GET /conversations` | Current user's Conversations, paged with `PageReq`, ordered by `updated_at` desc. No exchanges in the list items. |
| `GET /conversations/{id}` | One Conversation with all its Exchanges in display view (below). |
| `DELETE /conversations/{id}` | Deletes it; exchanges/messages cascade, `llm_calls` rows survive with nulled FKs. |
| `POST /conversations/{id}/exchanges` | Body `{ "content": "<user text>" }`. Starts an Exchange and returns an SSE stream. |
| `GET /conversations/{id}/exchanges/{exchange_id}/stream` | Rejoins an Exchange's stream (see *Rejoining*). SSE. |

Another user's Conversation behaves as not found (404) — *builder's call, mine*.

Errors that can be known before streaming starts (not found, empty content, an Exchange already in progress) return as ordinary JSON error responses, not as SSE events. Once the stream response starts, failures arrive as `exchange_failed`.

New `RuleViolation`s (400):
- `ConversationExchangeInProgress` — a message was posted while an Exchange in that Conversation is `in_progress`. The partial unique index is the real guard; a unique-violation on insert must map to this violation too.
- `ExchangeContentEmpty` — content empty or whitespace (*my call*).

Creating an Exchange bumps the Conversation's `updated_at` (*my call*).

## SSE wire format

Exactly three event types:

```
event: text_delta
data: {"block":0,"text":"The Thing (1982) is"}

event: exchange_completed
data: {"exchange_id":42}

event: exchange_failed
data: {"exchange_id":42,"key":"ProviderRateLimited","debug_message":"429 from OpenRouter: ..."}
```

- `block` is an **Exchange-wide** index over displayable blocks, not the provider's per-call `index`. In plain chat it is normally 0. Later, text after a tool round becomes the next block, and a Widget becomes its own block in between. Explicit block start/stop events were considered and rejected; the index carries the same information.
- No "message saved", no "exchange started" events.
- Exactly one terminal event (`exchange_completed` or `exchange_failed`) ends every stream that the server finishes. A stream that ends without one was cut off; the client rejoins (below).

## Display view (`GET /conversations/{id}`)

The client never sees raw Messages. Each Exchange is returned as:

```json
{
  "id": 42,
  "status": "completed" | "failed" | "in_progress",
  "created_at": "...",
  "completed_at": "...",
  "content": "<the user's text>",
  "blocks": [ { "type": "text", "text": "..." } ],
  "error": { "key": "ProviderRateLimited", "debug_message": "..." }   // only when failed
}
```

`blocks` uses **the same indexing as the stream**: the displayable content blocks of the Exchange's assistant Messages, in message order then block order, enumerated from 0. Today only `text` blocks are displayable; thinking (should it ever appear), tool_use and tool_result blocks are skipped. Later, widgets enter this list as `{ "type": "widget", ... }`. Live streaming and reloaded history must render with the same client code, so the derivation of `block` in the stream and in this view must be one shared rule — ideally one shared function.

Field names and exact envelope (e.g. whether `content` is called `user_text`) are the builder's call; the shape is not.

## The Exchange lifecycle

1. Endpoint validates, inserts the Exchange (`in_progress`) and the user Message (content `[{"type":"text","text":...}]`), bumps `updated_at`.
2. It creates the Exchange's entry in the **Exchange registry** (below) *before* submitting `Job::RunExchange`, so a subscriber can never arrive before the entry exists. The SSE response subscribes to that entry — the POST's response is simply the first subscriber.
   **Why background work:** Axum drops the response body stream when the client disconnects, and dropping a future cancels it at its next `.await`. If the Exchange ran inside the stream, closing a tab would cut it off mid-write and leave it `in_progress`. As a job, a disconnect just drops that subscriber; the Exchange carries on and finishes.
3. The job builds history, makes the LLM call, streams, persists, ends the Exchange and removes the registry entry.

### History sent to the model

- System prompt.
- The messages of every **completed** Exchange in the Conversation, in order, verbatim from `messages.content`. **Failed Exchanges are left out entirely** (they stay visible to the person). In-progress ones cannot exist besides the current one.
- The current Exchange's messages so far (today: just its user Message; with tools: the rounds already done).
- A prompt-caching breakpoint at the end of the history so repeated Exchanges re-read earlier turns from cache (*my call*; how exactly depends on what openrouter-rs exposes — `cache_control` on the last block is the usual way). Many open-model providers ignore `cache_control` or cache automatically; that's fine, it must just never cause an error, and cache token counts may be absent.

### The loop

Write the Exchange as a loop over LLM calls with a `match` on a typed **stop-reason enum** (`EndTurn`, `StopSequence`, `MaxTokens`, `ToolUse`, `Refusal`, unknown…). In this increment the loop runs once:

- `EndTurn` / `StopSequence` → Exchange completed.
- `MaxTokens` → keep the Message as-is, Exchange completed. (With tools later: if the cut lands inside a tool_use block, the Exchange fails with `OutputTruncated`; automatic continue/retry may go in that branch later.)
- `Refusal` → Exchange failed with `Refused`; whatever arrived stays as its Partial message.
- Any normal stop with no text produced → Exchange failed with `EmptyResponse`, so a user Message with no reply never enters history.
- `ToolUse` → the branch tools will fill. Today we send no tools so it cannot happen; treat as `Internal` if it somehow does.
- Unknown stop reason → complete, log a warning (*my call*).

### One LLM call

1. Insert the `llm_calls` row **before** sending (purpose `exchange`, user, conversation, exchange, model, `created_at`). Every request ever sent has a row.
2. Call `stream_messages` with `max_tokens` from config, no thinking, no tools.
3. Consume the Anthropic event stream through an **accumulator** — a small state machine that turns events into finished content blocks:
   - `message_start` → provider message id, input/cache token usage.
   - `content_block_start` / `content_block_delta` / `content_block_stop` → build blocks. Deltas are untyped JSON in openrouter-rs; handle `text_delta` now. Design it so `input_json_delta` (tool_use input, concatenated then parsed at block stop) and thinking deltas are one more arm later.
   - `message_delta` → stop reason, output tokens.
   - `message_stop` → OpenRouter metadata.
   - `ping` → ignore. `error` → mid-stream failure, classified exactly like an error response (below).
   - `thinking` blocks: we request no thinking, but some open models reason regardless. Store them verbatim in the Message, never display them, and don't count them as text for `EmptyResponse`. The assistant Message is created with the first non-empty content of any kind (text or thinking), so nothing that arrived is lost and no empty block is ever stored (*my call*).
   - The accumulator maps the provider's per-call `index` to the Exchange-wide `block`.
4. On completion or failure, update the `llm_calls` row: stop reason, token counts, provider request id, error (the provider's full error response), `completed_at`.

### Write first, forward second

Client state must equal DB state:

- The assistant Message row is created with the **first non-empty content** — text or thinking — (with its `llm_call_id`), never earlier, so no stored Message ever holds an empty block, which the Anthropic format rejects on replay. A call that fails before any content produces **no** Message. Each LLM call produces at most one assistant Message. (With tools later: the first non-empty displayable or tool content.)
- Every text chunk is written to `messages.content` **before** it is sent as a `text_delta`.
- Deltas that arrive while a write is in flight are coalesced into the next write, and the client gets that same coalesced chunk. The client is never ahead of the DB; DB write rate is bounded by DB latency, not token rate. (Mechanism is the builder's call; one way: after each event, drain whatever is already buffered from the provider stream before writing.)
- `exchange_completed` / `exchange_failed` are sent only after the Exchange's status is committed.

## Rejoining

A client that reloads or loses its connection mid-Exchange must be able to pick the live stream back up, rendering with the same code as a fresh stream.

- **Exchange registry**, an in-process structure owned by `AppState` and passed explicitly like the other dependencies (no global), keyed by Exchange id, holding for each running Exchange: the blocks streamed so far (exactly what has been committed to the DB) and its subscribers. Entry removed when the task ends.
- `GET /conversations/{id}/exchanges/{exchange_id}/stream` subscribes: it first **replays** the stored blocks as `text_delta` events (one per block is fine), then continues with live events, then the terminal event. Still only the three event types.
- **No gaps, no duplicates:** taking the snapshot and registering the subscriber must be atomic with respect to the task's "commit, then forward" step (e.g. both under the entry's lock). Because forwarding only happens after commit, the snapshot equals DB state at that instant.
- **Already finished** (not in the registry): replay from the DB and send the terminal event straight away (`exchange_completed`, or `exchange_failed` with the stored error). The client then never needs a special case for "finished while I was away" (*my call*). Handle the race where the task finishes between lookup and DB read by falling back to the DB path.
- Unknown Exchange or another user's → 404.
- A stop/cancel action is **not** in this increment, but the registry entry is where its cancel handle will live; keep room for it.

## Failures

A failure ends the Exchange as `failed`. If output had begun, the assistant Message stays as a **Partial message** with whatever was stored. Nothing is deleted. A future Retry clears it (deletes the Partial message, redoes the call; never touches `llm_calls`).

Typed failure enum (keys rendered like `RuleViolation`, via `Debug`).

**Classification is model-independent** and never matches message text. Two signals, depending on when the error happens:

- **Before streaming** (an HTTP error status): classify on the **status**. openrouter-rs 0.22 parses the error body into its own `ApiErrorContext` and drops OpenRouter's canonical `error.error_type`, so the status is all we reliably have (`ApiErrorContext.status`, plus its `message` and `metadata` for `debug_message` and the ledger).
- **During streaming** (an SSE `error` event; HTTP status was already 200): openrouter-rs passes the error object through as raw JSON, so classify on OpenRouter's canonical **`error.error_type`**.

Whether the error came before or during streaming only decides the signal and whether a Partial message exists; the keys are the same.

| Key | HTTP status (before streaming) | `error_type` (during streaming) |
|---|---|---|
| `ProviderRateLimited` | 429 | `rate_limit_exceeded` |
| `ProviderUnavailable` | 5xx, 408; network errors and timeouts (no status); stream idle ~60 s (openrouter-rs sets no timeout, so we add one) | `provider_overloaded`, `provider_unavailable`, `server`, `timeout` |
| `ProviderOutOfCredits` | 402 | `payment_required` |
| `ProviderAuthFailed` | 401, 403 | `authentication`, `permission_denied` |
| `Refused` | — | `refusal`, `content_policy_violation` — and also the stop reason `refusal`. A failure so the refused turn is left out of history |
| `ProviderError` | anything else, e.g. 400 | anything else, including unknown future types |
| `EmptyResponse` | — the call ended normally but produced no text | |
| `Interrupted` | — Exchange was `in_progress` when the server stopped (see below) | |
| `Internal` | — our side failed (DB, serialization, unexpected stop reason) | |

`ProviderError`'s `debug_message` starts with the status (e.g. `400: <provider message>`) so the person sees it. A conversation that outgrew the model's context window is just such a 400 for now — no dedicated key; compaction will handle long conversations later (*user's decision*).

`ProviderOutOfCredits` and `ProviderAuthFailed` are split out because they tell the person what to do (top up, fix the key) (*my call*). `error_type` is an open set; parse it as a string and map unknown values to `ProviderError` with the raw value in `debug_message`.

Docs for the envelope: OpenRouter "Errors and debugging" (`/docs/api/reference/errors-and-debugging`). Errors after routing carry a `request_id` (`gen-…` generation id) — use it for `llm_calls.provider_request_id` when present.

Reserved for the tools increment, not added now: `OutputTruncated`.

Where it is stored (both JSONB):
- `exchanges.error` holds `{"key": ..., "debug_message": ...}`, so the display view and the SSE event come from one place and `Internal` failures (no provider response) have somewhere to keep their detail.
- `llm_calls.error` holds `{"status": <HTTP status or null>, "message": "<human-readable summary>", "response": <the provider's raw JSON error body or in-stream error event, or null>}`. The raw provider JSON is kept untouched under `response`; transport failures (no response at all, timeouts) have only `status`/`message` (*my call*).

The `exchange_failed` event and the display view's `error` field have the same `{key, debug_message}` shape.

### Interrupted Exchanges

On server start, before serving requests, mark every `in_progress` Exchange as `failed` with key `Interrupted`, and set `completed_at`. Their Partial messages (already persisted, thanks to write-first) stay. Safe because this server is the only thing that runs Exchanges. Also close any `llm_calls` row of those Exchanges still lacking `completed_at` with an error noting the interruption (*my call*).

### Deleted mid-Exchange

A Conversation can be deleted while its Exchange runs (cascade). The task's next DB write then fails; it must treat that as cancellation: stop reading the provider stream, finish the `llm_calls` row with an error saying the conversation was deleted, and exit quietly (*my call*).

## Where things go (suggested, builder may adjust)

- `integrations/src/openrouter/` — the `Llm` trait, `LlmRequest`/`LlmEvent`, and `OpenRouterClient` wrapping openrouter-rs (request building, delta parsing, error capture). No DB.
- `services/src/services/conversations.rs` (+ helpers) — CRUD, Exchange lifecycle, history building, accumulator, persistence, failure mapping, startup recovery; the `RunExchange` job handler lives here, dispatched from `services::jobs::run`.
- `services/src/test_support/llm.rs` — `MockLlm` and event/error builders; chat model builders go in `test_support::fixtures`.
- `views/src/conversations/` — request/response DTOs, SSE event payloads, display view.
- `api/src/endpoints/conversations.rs` — handlers, SSE response (`axum::response::sse`), OpenAPI annotations; routes in `routes::build()`, paths in `openapi.rs`.

## Testing

Follow `CLAUDE.md`'s testing rules exactly (placement, `<function>_<expected outcome>_when_<condition>` names, `init()` first, mock-DB queuing and assertion rules, ignore reasons, `--no-fail-fast` with `--ignored`). After changes: `cargo test --workspace`, `cargo clippy --workspace --all-targets` with no new warnings, and — because this touches a migration and SQL — the Postgres suite.

**Unit tests** (`#[cfg(test)] mod tests` in the file under test; mock DB, `MockLlm`, `RecordingJobs`, `FixedClock`):
- Pure parts: accumulator (events → blocks, provider index → Exchange-wide `block`, state at the moment of failure, no Message before the first non-empty content, thinking stored but not displayed), stop reason → outcome (`Refused`, `EmptyResponse`, `MaxTokens` completes), error classification (status → key before streaming, `error_type` → key during streaming, unknowns → `ProviderError`), display-view block derivation shared with the stream, history building (failed Exchanges skipped).
- Registry: snapshot-plus-subscribe is gap-free and duplicate-free against concurrent commit-then-forward.
- Service functions: conversation CRUD and ownership (404 for another user's), `start_exchange` violations (`ExchangeContentEmpty`, `ConversationExchangeInProgress`), the job is submitted, the `RunExchange` handler's writes for success / partial / pre-output failure (assert `assert_wrote` where the write is the behaviour), the idle timeout (paused Tokio time).
- `integrations`: `OpenRouterClient`'s translation of openrouter-rs events and errors into `LlmEvent`/`Error`, in-module, without network.

**Postgres tests** (`services/tests/postgres_chat.rs`, `#[ignore = "requires docker"]`): only for what the mock DB cannot prove — the partial unique index surfacing as `ConversationExchangeInProgress`, the startup-recovery SQL, the job stopping quietly when its Conversation is deleted mid-Exchange, and one end-to-end run tying the pieces to a real database. Environment-dependent tests exist to link the code to a real environment, not as the main kind of test.

**Live OpenRouter contract test** (`integrations/tests/openrouter_contract.rs`, like `tmdb_contract.rs`): exactly one, against the real API with `OpenRouterClient`, reading `openrouter.api_key` from `config.yaml` but always using the model **`openrouter/free`** (OpenRouter's Free Models Router — zero cost; it picks a random free model per request). It is `#[ignore = "requires OpenRouter key"]`, a new ignore reason to add to `CLAUDE.md`'s list, together with the run command (`cargo test -p integrations --test openrouter_contract -- --ignored`).

- A short prompt streams into `LlmEvent`s ending with some text and a stop reason — checks our translation against the real wire format.

Assertions must not depend on which model answered or on exact text. Free endpoints are rate limited, so a 429 makes the test fail rather than pass silently.

## Left to the builder

Module layout, DTO field names, coalescing mechanism, registry/channel types and capacity, how to express the SSE endpoint in utoipa, how prompt caching is expressed through openrouter-rs, which provider id goes into `provider_request_id` (Anthropic message id vs OpenRouter generation id — prefer the one OpenRouter's dashboard can look up).

## Calls I made (check these)

- Another user's Conversation → 404.
- `ExchangeContentEmpty` violation for blank content.
- Creating an Exchange bumps `updated_at`.
- Prompt caching on by default.
- Unknown stop reasons complete the Exchange (with a warning).
- Assistant Message is created on the first non-empty content, not on the first block start; `EmptyResponse` for a call that produced no text.
- ~60 s idle timeout on the provider stream, reported as `ProviderUnavailable`.
- Rejoining a finished Exchange replays it from the DB and ends with its terminal event, rather than erroring.
- `exchanges.error` stores `{key, debug_message}` rather than just the key with the debug message joined from the ledger — one source for both the event and the view, and a home for `Internal` failures.
- `llm_calls.error` envelope `{status, message, response}`, so transport failures (no provider body) fit the same JSONB column as provider errors.
- The JSONB change edits the unmerged chat migration in place instead of adding a new one.
- Interrupted Exchanges also close their dangling `llm_calls` rows.
- `Llm` trait speaks our own `LlmRequest`/`LlmEvent` types, not openrouter-rs types.
- Parameter order `db, llm, registry, tmdb, store, jobs, clock, random`.
- One live contract test (streaming), on `openrouter/free`.
- Failures are classified on HTTP status before streaming and on OpenRouter's `error_type` during streaming; `ProviderRejected` and `ProviderStreamError` are replaced by `ProviderError`, and `ProviderOutOfCredits` / `ProviderAuthFailed` are new.
- Thinking blocks from models that reason anyway are stored but not displayed; the Message is created on the first non-empty content of any kind.
- Deleting a Conversation mid-Exchange cancels the task quietly.
