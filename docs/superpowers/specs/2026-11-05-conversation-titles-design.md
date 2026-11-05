# Conversation Titles — design

Date: 2026-11-05
Builds on: `2026-10-29-llm-chat-endpoints-design.md` (plain chat), `2026-10-31-chat-tools-design.md`, `2026-10-31-reasoning-design.md` and `2026-11-02-exchange-retry-design.md`, all merged to master. Domain terms are in `CONTEXT.md`; use them exactly.

## Intent

A Conversation's title is today always null: the schema has `conversations.title` and the `title` value of `llm_call_purpose`, and `CONTEXT.md` already says "a Conversation's title is produced by its own LLM call, outside any Exchange", but nothing produces one. The person's conversation list is therefore a column of anonymous rows. This feature has a cheap model write a short title from the person's first message, in parallel with the answer, and tells the client the moment it exists.

To make that possible without a new kind of connection, **creating a Conversation now takes the first message and streams**. The creating request's response is the one place the client is guaranteed to be listening when the title lands, so the title is delivered there. A new `stream_end` event, sent last on every SSE stream whenever the server ends it, gives the client one unambiguous "nothing more will come" signal.

The title is **independent of the Exchange**. The Exchange runs, completes or fails, closes its registry entry and can be retried exactly as before. The title has its own job and its own channel. Only the creating HTTP response is held open longer, to carry the title.

## Goals

- Every Conversation created through the API gets a short title unless titling is switched off or the title call fails.
- The title is produced by a model different from, and cheaper than, the main one, chosen in `config.yaml`.
- The title does not delay the answer and the answer does not depend on the title: the title call and the Exchange run in parallel and never wait for each other.
- The client learns the title live, on the creating request's stream, and always finds it in the database afterwards (client state = DB state; the event is written after the DB write, like every other event).
- Every SSE stream the server ends ends with `stream_end`, whatever the reason, so the client has one signal to close the connection on.
- Every title request is a permanent `llm_calls` row, successful or not.

## Non-goals

- Renaming a Conversation, regenerating a title, or retrying a failed title call. Retrying the first Exchange never reruns the title.
- Titling Conversations that already exist (they keep a null title) or titling from later messages or the answer.
- Pushing a title to any stream other than the creating request's. Rejoin, add-Exchange and retry streams never carry a `title`.
- A per-user or per-conversation notification stream.
- A timeout of our own on the title call beyond the 60 s stream idle timeout the Exchange already uses.
- Gating anything on the title: a failed title never affects the Exchange.

## Anti-goals

- The title call delaying, failing or being visible in the Exchange (its LLM call has no `exchange_id`, and a title failure never appears as an Exchange error).
- Any coupling of the Exchange to the title: the Exchange's run, registry entry, `is_open` check and Retry must behave exactly as they do today, with the title job pending or not.
- A creating stream that never ends because the title model is broken, unconfigured or its job died.
- Treating `stream_end` as "everything went well". It only means the server has ended the stream. Whether the run was healthy is read from the events before it (`exchange_completed` or `exchange_failed` present or not).
- A new migration. Everything needed exists.

## Constraints

- All constraints of the earlier specs hold: strict layering, SeaORM queries only in `services`, outside-world access only through the injected traits (`Llm`, `Jobs`, `Clock`, `Random`), the `CLAUDE.md` testing rules and the services parameter-order rule, model-independent behaviour (open models; never rely on model-specific output).
- Background work goes through `Jobs`; no direct spawning.
- Write to the database before publishing the matching event. Fixed query count, no per-item loops.
- `openrouter-rs` stays the only OpenRouter client; the title model uses a second `OpenRouterClient` built the same way as the main one.
- One `in_progress` Exchange per Conversation (existing partial unique index) is unaffected.

## The design

### Creating a Conversation

`POST /conversations` takes `ExchangeCreate` (`{ "content": "..." }`) and replaces the old empty create (breaking change to that endpoint; nothing else creates Conversations). It behaves like adding an Exchange to a fresh Conversation:

1. Empty or whitespace-only content is rejected with `ExchangeContentEmpty` as an ordinary error response, before anything is created.
2. In one transaction: insert the Conversation (`title` null), its first Exchange (`in_progress`) and the user Message.
3. Open the Exchange's registry entry and subscribe to it, submit `Job::RunExchange`, and if titling is enabled open the title channel and submit `Job::GenerateTitle`.
4. Respond with an SSE stream (`200`, `text/event-stream`), like the other exchange endpoints.

Errors that occur before the stream starts are normal JSON error responses, as for the other exchange endpoints.

### Events

Three new SSE events. Payloads are JSON like the existing ones.

| Event | Payload | Sent on |
|-------|---------|---------|
| `conversation_created` | `{ "conversation_id": int, "exchange_id": int }` | The creating stream only, always first. It is part of what the creating request is given, not stored in the registry, so a later rejoin does not replay it. |
| `title` | `{ "title": string }` | The creating stream only, after the title is written to `conversations.title`. At most once. |
| `stream_end` | `{}` | Every SSE stream the server ends, always last. |

The creating stream is the Exchange's events (blocks, then `exchange_completed` or `exchange_failed`) followed by the title's, then `stream_end`:

1. `conversation_created`
2. the Exchange's events up to and including its terminal event
3. `title`, if one was generated. The title may have finished before the Exchange; it is then held and sent after the terminal event, so the order of the stream is always Exchange first, title second.
4. `stream_end`

The stream's own end of life is what produces `stream_end`: when the Exchange's source has ended **and** the title's source has ended (published, failed, or never opened), the response sends `stream_end` and closes. This holds whatever the reason the sources ended, including a run that panicked, a title job that panicked, or a Conversation deleted mid-run. In those cases there is simply no terminal event or no `title` before the `stream_end`.

On every other stream (add Exchange, retry, rejoin) `stream_end` follows `exchange_completed` or `exchange_failed`, or follows the end of the Exchange's source however it ended.

A rejoin or retry of an Exchange with a closed registry entry is rebuilt from the database as today and ends with `stream_end`. This includes an `in_progress` Exchange with no entry (should not happen after startup recovery): it keeps today's behaviour of no terminal event, then `stream_end`.

`stream_end` cannot be sent when the process is killed or the network drops. A client that sees the connection end without `stream_end` treats that as abnormal.

The Retry guard is unchanged: the first Exchange's registry entry closes when its run ends, so a failed first Exchange can be retried at once, even while its title is still being generated. That Retry's stream carries no `title` and does not touch the title job.

### The title job

New `Job::GenerateTitle { conversation_id }`.

- Input: the text of the first user Message of the Conversation (read from the database by the job, not carried in the job), sent as a single user message with a title-only system prompt kept in a `.md` file next to `system_prompt.md`. No tools, no history.
- Output handling: concatenate the text blocks of the answer; trim, collapse whitespace, strip surrounding quotes, cap the length. An empty result counts as failure.
- Success: update `conversations.title` (not `updated_at`, so a title does not reorder the list), then publish `title` to the title channel.
- Every request records an `llm_calls` row with purpose `title`, the title model, `user_id` and `conversation_id`, `exchange_id` null, then completed with stop reason and token usage or with the error, exactly as Exchange calls do. If the Conversation was deleted meanwhile, the row is still written (detached by the existing foreign keys) and nothing is published.
- Failure of any kind (provider error, mid-stream error, idle timeout of 60 s, empty result, database error) is logged, leaves the title null and publishes nothing. It never reaches the Exchange.
- Whatever happens, including a panic, the title channel is closed when the job ends, so the creating response is never held open by a dead job. A drop guard like `CloseOnDrop`, or dropping the sender, does it.
- The title call has no timeout of its own beyond the 60 s stream idle timeout the Exchange uses (decided: 60 s is acceptable).

### Configuration

New optional `openrouter.title_model`. Absent: titling is off, no job is submitted, no title channel is opened. Present: a second `OpenRouterClient` with that model, the same `api_key` and `data_policy`, **no reasoning**, and a small `max_tokens`. `JobDeps` gains the optional title `Arc<dyn Llm>`; the real wiring builds it next to the main client. The model must differ from `openrouter.model`; the configuration check should reject an equal value.

### Registry

The Exchange's entry and its lifecycle do not change, except that `stream_end` now exists as the response's last event (see Events). The title gets its own small channel, separate from the Exchange's entry, that the creating request subscribes to and the title job publishes to and closes. It is keyed in whatever way the builder finds natural (the Conversation id is the obvious key). The creating response chains the two sources. A title published before the creating request subscribes must not be lost, and a title channel that nobody subscribed to must not leak. Both are the builder's to get right.

## Documentation to update

`CONTEXT.md` (the creation step, a note that a title is delivered on the creating stream), the OpenAPI descriptions of the four SSE endpoints (event lists, `stream_end`) and of `POST /conversations`, and `CLAUDE.md`'s config reference for `openrouter.title_model`.

## Left to the builder

- The title prompt wording, maximum title length and `max_tokens` (a title is a few words; the prompt should ask for the language of the message).
- The shape and key of the title channel, and how the response chains the Exchange and title sources.
- The `stream_end` / `title` / `conversation_created` view types and their place in `ExchangeEvent`.
- Test layout, following the `CLAUDE.md` rules. At minimum: both finish orders for Exchange and title, title failure, title job panic, titling off, deleted Conversation, retry of the first Exchange while the title is pending, `stream_end` on every endpoint, and a real-Postgres test for the creation transaction.

## Calls made while writing this (not yet confirmed by the person)

- The creating response holds the title back until after the Exchange's terminal event, so the order is always Exchange then title, and `stream_end` closes it.
- `stream_end` is sent whenever the server ends the stream, including after an abnormal run, and does not mean the run succeeded.
- `conversations.updated_at` is not touched by the title write.
- The title call's `llm_calls` row has `conversation_id` but no `exchange_id`, following `CONTEXT.md`.
- No stream other than the creating one ever carries a `title`; a rejoin does not.
- The title model must differ from the main model, enforced in the configuration check.
- Titling being switched off is expressed by leaving `title_model` out.
- A failed first Exchange still gets a title.
