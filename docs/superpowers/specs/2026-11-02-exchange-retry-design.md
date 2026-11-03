# Exchange Retry — design

Date: 2026-11-02
Builds on: `2026-10-29-llm-chat-endpoints-design.md` (plain chat), `2026-10-31-chat-tools-design.md` (tools) and `2026-10-31-reasoning-design.md`, all merged to master. Domain terms are in `CONTEXT.md`; use them exactly.

## Intent

An Exchange that failed stays visible to the person with its error and any Partial message, and is left out of the history sent to the model. Today the only way forward is to send a new message, which abandons the failed one. **Retry** lets the person resume a failed Exchange from the point it failed, keeping the Messages that already succeeded (user Message, finished tool rounds), so a transient failure (rate limit, provider outage, server restart) costs one click, not the whole answer.

Retry is the first of the "Retry / Regenerate / edit" family that the first two specs deferred. Regenerate and edit/Truncate stay out.

## Goals

- Retry any failed Exchange of the user's, whatever its failure key.
- The retried Exchange moves to the end of the Conversation, so history stays a single line and nothing after it is deleted or changes meaning.
- Client state = DB state still holds: the Partial message is deleted before the client is told anything, and the retry stream replays what was kept.
- A Retry cannot earn a fresh tool round budget.
- No `llm_calls` row is ever deleted or detached; the failed call stays in the ledger.

## Non-goals

- Regenerate (rerun from the user Message) and editing an earlier user Message (Truncate).
- Gating Retry by failure key (e.g. refusing it for `Refused`). The person decides.
- Automatic retry, a retry counter or limit, back-off.
- Stopping an Exchange mid-stream.
- A new job variant. `Job::RunExchange` is reused.

## Anti-goals

- Deleting and re-inserting the Exchange row. That would change `exchange_id`, which the client holds, and null `llm_calls.exchange_id`.
- Any Retry that leaves a history the model cannot accept (an assistant `tool_use` with no `tool_result`).
- The client ever seeing a block that is not in the database, or a gap or duplicate between the replay and the live events.

## Constraints

- All constraints of the earlier specs still hold: strict layering, SeaORM queries only in `services`, outside-world access through the injected traits (`Clock` for every timestamp, `Jobs` for the run), the `CLAUDE.md` testing rules, model-independent behaviour.
- **No migration.** Ordering uses existing columns.
- One `in_progress` Exchange per Conversation stays enforced by the partial unique index `idx_exchanges_one_in_progress`.
- Database access has a fixed query count, never per-item loops (user's standing rule).

## Behaviour

### Endpoint

`POST /conversations/{conversation_id}/exchanges/{exchange_id}/retry`, no body, in the user router, documented in OpenAPI. Path params can reuse `ExchangeStreamParams`. It answers like `create_exchange`: SSE of `thinking_delta`, `text_delta`, `tool_call`, then `exchange_completed` or `exchange_failed`. Errors known before streaming are ordinary JSON errors:

| Case | Response |
|---|---|
| Conversation or Exchange not found, or another user's, or the Exchange belongs to another Conversation | 404 |
| The Exchange is not `failed` (`completed` or `in_progress`) | 400, new `RuleViolation::ExchangeNotFailed` (*my call*) |
| Another Exchange of the Conversation is `in_progress` | 400, existing `ConversationExchangeInProgress` |
| Its registry entry is still open (see *Registry*) | 400, `ConversationExchangeInProgress` |

### What Retry does

Service function `retry_exchange` next to `start_exchange` in `services/src/services/chat/conversations.rs` (parameter order per `CLAUDE.md`: own parameters, then `db, registry, jobs, clock`).

1. Check ownership, then that the registry entry for the Exchange is not open (rejected as above if it is).
2. In **one transaction**:
   - Conditional update: `status = in_progress`, `error = NULL`, `completed_at = NULL`, `created_at = clock.now()` `WHERE id = ? AND status = 'failed'`. Zero rows affected means the Exchange was not failed (or was retried concurrently): `ExchangeNotFailed`. A unique violation on `idx_exchanges_one_in_progress` maps to `ConversationExchangeInProgress`, as `insert_exchange` already does through `conflict_or_db`.
   - Delete the Exchange's trailing Message **if it is an assistant Message**, and only then: `DELETE FROM messages WHERE id = (SELECT max(id) FROM messages WHERE exchange_id = ?) AND role = 'assistant'`. This one rule covers every case:
     - the Partial message of the failed LLM call (text, thinking, or a `tool_use` cut off mid-stream);
     - an assistant Message that asked for tools whose tool round then failed (`Internal`, deleted Conversation races, `ToolRoundLimit` after the round was refused): its `tool_use` blocks have no `tool_result`, so the Message must go and the call is made again;
     - a Message left by `Refused` or `EmptyResponse`.

     A history valid for the model ends in a user or tool Message, so after this step it does. Older assistant Messages are never touched.
   - Bump the Conversation's `updated_at` to the same instant.
   - Load the Exchange's remaining Messages (for the registry seed below).
3. After the commit: open the registry entry seeded with `display_blocks(kept Messages)` and subscribe to it (so the response's first subscriber cannot miss anything), then submit `Job::RunExchange { exchange_id }`. Return the subscription.

`llm_calls` rows are not touched. The deleted Message's call row keeps its `stop_reason`, usage and `error`, and loses nothing (`messages.llm_call_id` points from the Message to the call, not the other way).

### Position: the Exchange moves to the end

Setting `created_at` to now (user's decision, 2026-11-02, over adding an ordering column) moves the Exchange to the end, provided Exchanges sort by `(created_at, id)`. Today two queries sort by `id` and must change:

- `details` in `conversations.rs` (the Conversation view),
- `load_history` in `exchange.rs` (the history the model sees; `build_request` keeps the order it is given).

`created_at` of an Exchange therefore means "when its current attempt started". The Conversation view shows the retry time. The original creation time is not kept (accepted). Messages within an Exchange keep ordering by `id`; Retry never reorders them. The client sees the Exchange jump to the end of the Conversation, and handles that itself.

### Registry

- `ExchangeRegistry::open` starts with no blocks. Add a way to open an entry **with** blocks (`open_with(exchange_id, blocks: Vec<DisplayBlock>)`, name the builder's call) so a stream that starts or rejoins mid-retry replays the kept blocks before the live ones. The retry response itself gets the same replay: the client resets the Exchange's blocks to it, which also removes the old Partial message from its display. Still only the existing event types; no "retry started" event.
- **Stale-entry race.** A run closes its entry (`CloseOnDrop`) only after its terminal status has committed and its terminal event is published, so there is a window where the database says `failed` while the entry is still open. Retry must not open an entry over it: the old run's remaining `publish` and its `close` look the entry up by id and would hit the new one. Hence step 1: if the entry is open, reject with `ConversationExchangeInProgress` before touching the database. Once it is seen closed, nothing else can reopen that id except this Retry, so the check cannot be invalidated before step 3.

### Running it

`Run::exchange` already starts from stored Messages: it loads the history, finds the current Exchange in it, and numbers new blocks after `display_blocks` of the Exchange's Messages. So Retry needs no new flow, only one change:

- **Tool round budget from the database.** `Run.rounds` and `Run.refused` start at 0/false today. At the start of `Run::exchange`, derive them from the Exchange's stored Tool Messages: with `n` of them, `rounds = min(n, max_tool_rounds)` and `refused = n > max_tool_rounds` (a refused round stores a Tool Message but does not count as a round, so a refused Exchange holds `max + 1`). For a new Exchange `n = 0`, so nothing changes. Result: a Retry never grants a fresh budget, and a `ToolRoundLimit` Exchange fails again at once if the model asks for tools again, while a model that now answers completes.

`build_request` is unchanged: it keeps completed Exchanges and the current one, in the order given, and the retried Exchange is now last.

## Docs to update

- `CONTEXT.md`: **Retry** gains "and the Exchange moves to the end of the **Conversation**" (and says its `created_at` becomes the retry time if that is worth a line).
- `docs/superpowers/specs/2026-10-29-llm-chat-endpoints-design.md` is not edited; this document supersedes its "Retry is a non-goal" line.
- OpenAPI path registration in `api/src/openapi.rs`, route in `api/src/config/routes.rs`.

## Testing

Follow `CLAUDE.md` exactly. The main kind is unit tests with the mock DB, `MockLlm`, `RecordingJobs` and `FixedClock`; real Postgres only for what the mock cannot prove.

**Unit** (`#[cfg(test)] mod tests` in the file under test):
- `retry_exchange`: not found for another user's Conversation and for an Exchange of another Conversation; `ExchangeNotFailed` for `completed` and `in_progress` (zero rows affected); `ConversationExchangeInProgress` when the registry entry is open (no statements executed) and on the index conflict; success submits `Job::RunExchange`, the subscription replays the kept blocks and is live, and `assert_wrote` shows the status update, `created_at` from the clock, and the Conversation `updated_at`.
- The trailing-Message rule: assistant Message last → deleted (also `tool_use`-only); tool or user Message last → nothing deleted.
- The derivation of `rounds`/`refused` from Tool Message counts: 0, below, at, and above the limit, with `config().assistant.max_tool_rounds`.
- `run_exchange` after a Retry: block numbering continues after the kept blocks; at the limit a model that asks for tools again ends `ToolRoundLimit`.
- Registry: `open_with` replays the seeded blocks, then live events, without gaps or duplicates.
- Ordering: `details` and `load_history` sort by `(created_at, id)`: assert on the generated `ORDER BY`, never full SQL strings.

**Postgres** (`services/tests/postgres_chat.rs`, `#[ignore = "requires docker"]`), only: the Retry transaction (the conditional update, the single-statement trailing delete, the index conflict with another `in_progress` Exchange, rollback if a step fails, using a trigger as in `postgres_transactions.rs`), the `(created_at, id)` ordering moving a retried Exchange behind a newer one, and one end-to-end Retry with `InlineJobs` and a `MockLlm`: fail, retry, complete, check that the history sent to the model on the retry ends with the right Message and that `llm_calls` kept both calls.

After the change: `cargo test --workspace`, `cargo clippy --workspace --all-targets` with no new warnings, and the Postgres suite (`cargo test -p services --no-fail-fast --tests -- --ignored`), since SQL and a transaction are touched.

## Left to the builder

Module layout, the registry method name and signature, the exact violation text, whether ownership and Exchange lookup are one query or two, how the retry endpoint shares code with `create_exchange`'s SSE response, test fixtures and `test_support` helper names.

## Calls I made (check these)

- Any failure key is retryable; no gating.
- `ExchangeNotFailed` as a new 400 violation for an Exchange that is not failed.
- Retry bumps the Conversation's `updated_at`, like creating an Exchange.
- The trailing assistant Message is deleted by one rule (last Message, if assistant), rather than per failure key.
- `rounds`/`refused` are derived from stored Tool Messages, so Retry cannot restore the tool budget.
- An open registry entry rejects the Retry as `ConversationExchangeInProgress` instead of replacing the entry.
- The retry stream replays the kept blocks, with no new event type; the client resets that Exchange's blocks on starting a Retry.
- No new job variant; `RunExchange` is reused.
- Zero rows affected by the conditional update is reported as `ExchangeNotFailed`, even if the Conversation was deleted in the instant after the ownership check (a window of milliseconds; a 404 there is not worth an extra query).
- When no blocks were kept (a failure before any output), the retry response replays nothing and goes straight to live events; the client's reset of the Exchange's blocks covers it.
