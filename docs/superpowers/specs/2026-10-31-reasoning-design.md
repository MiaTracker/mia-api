# Reasoning models in conversations — design

Date: 2026-10-31
Builds on: `2026-10-29-llm-chat-endpoints-design.md` (plain chat, merged in `0ca44bc`). Domain terms are in `CONTEXT.md`.

## Intent

Mia's chat runs open models through OpenRouter's Anthropic-compatible Messages endpoint. Today every request sends `thinking: disabled`; thinking blocks that models produce anyway are stored but never shown. This increment turns reasoning on through configuration and shows the person the model's reasoning, live and on reload, alongside the answer it led to.

## Goals

- Reasoning is switched on and tuned in `config.yaml`, one setting for everyone, like the model.
- The person sees reasoning traces in the stream and in the Conversation view, in the order they happened, rendered by the same client code live and on reload.
- Reasoning from earlier Exchanges is not sent back to the model.

## Non-goals

- Per-Conversation or per-Exchange reasoning choice from the client. No API request changes.
- Fixing openrouter-rs's thinking-block parsing (see *Known gap*).
- `redacted_thinking` blocks: unchanged from today (ignored by the accumulator, not stored, not shown). Revisit with tools (*my call*).
- Separate reasoning token counts in `llm_calls` (the Anthropic usage format has none).

## Anti-goals

- The client ever seeing reasoning that is not in the database. Thinking follows the same write-first-forward-second rule as text.
- A second numbering or a second rendering path for thinking. Thinking is a block in the one Exchange-wide block list.
- Anything that depends on a particular model's reasoning behaviour. Models will change; some reason without being asked, some ignore the request, some send no signature.
- Hand-rolling an OpenRouter client to work around the crate.

## Constraints

- All constraints of the endpoints design still hold (layering, openrouter-rs 0.22, verbatim Anthropic content in `messages.content`, the `CLAUDE.md` testing rules, model-independent error classification).
- Stay on **openrouter-rs 0.22**. 0.23.0 (2026-10-30) does not fix the signature issue and is a breaking release; upgrading is out of scope (*my call*).
- No migration. Thinking is already stored verbatim in `messages.content`.

## Configuration (user's decision: config, not per request)

Optional `reasoning` section under `openrouter` in `config.yaml`, `infrastructure::config` and the config block in `CLAUDE.md` / `config.yaml.example`:

```yaml
openrouter:
  api_key: ...
  model: ...
  max_tokens: 16384
  reasoning:            # optional; absent → the request sends thinking disabled, as today
    effort: medium      # required when `reasoning` is present: low | medium | high | xhigh | max
```

- **`effort` replaces `budget_tokens`** (*user's decision, 2026-11-02, reversing the original "only `budget_tokens`" decision*). The original reason was that OpenRouter silently ignores parameters a model doesn't support, so an effort setting can't be verified to do anything; a model that ignores effort thinks however it likes. Accepted values are the full set `openrouter-rs` offers: `low | medium | high | xhigh | max`. There is no token budget and no `budget_tokens < max_tokens` startup check any more.
- Mapping onto the request, in `OpenRouterClient::build_request`:
  - absent → `AnthropicThinking::Disabled` and no `output_config` (today's behaviour).
  - `effort: e` → `AnthropicThinking::Adaptive` plus `output_config: { effort: e }`.
- `OpenRouterClient::new` takes the reasoning setting; the `Llm` trait and `LlmRequest` stay unchanged (reasoning is a client setting, like `max_tokens`).

### The setting only shapes the request

Everything else in this design is **unconditional**, independent of whether `reasoning` is configured (*my call*): thinking is displayed whenever it exists (including thinking models produce unasked, and thinking already stored in past Exchanges, whose block numbering therefore shifts to include it), and earlier Exchanges' thinking is always stripped from history. So `display.rs`, the accumulator, the registry and `history.rs` take no config, and stream and reload always agree, even across a config change between restarts.

## Display: thinking is a block (user's decision)

### Stream

A fourth SSE event:

```
event: thinking_delta
data: {"block":0,"text":"The user wants a horror film from the 80s…"}
```

- `block` is the same Exchange-wide index as `text_delta`. A thinking block and the text after it are different blocks: typically thinking is block 0, the answer block 1.
- A block number is assigned to a thinking block on its first non-empty thinking delta, exactly as for text today.
- Written to `messages.content` before it is forwarded; coalesced with whatever else is buffered, as text is.
- `signature_delta` is stored, never forwarded.
- Event set is now four: `text_delta`, `thinking_delta`, `exchange_completed`, `exchange_failed`. Still no block start/stop events.

### Display view (`GET /conversations/{id}`)

`DisplayBlock` gains a variant:

```json
"blocks": [
  { "type": "thinking", "thinking": "The user wants…" },
  { "type": "text", "text": "Try The Thing (1982)." }
]
```

- `display_blocks` includes thinking blocks with non-empty `thinking`, in message order then block order, alongside text blocks with non-empty text. The stream's numbering and this list must stay one shared rule (`display.rs`), and the existing test that reloads accumulated content and compares it with the streamed blocks must cover thinking.
- Blocks are listed in the order the provider sent them. The accumulator already numbers by first non-empty delta while `content()` keeps provider order; these only diverge if blocks interleave, which the Anthropic format does not do. Keep that as is.

### Rejoining

The Exchange registry currently holds each block as a `String`. It must also hold the block's kind, so a rejoin replays thinking as `thinking_delta` and text as `text_delta`. The replay-from-DB path (finished Exchange) uses `display_blocks`, so it gets thinking for free.

## History sent to the model (user's decision: strip earlier reasoning)

In `history::build_request`:

- Messages of **earlier completed Exchanges** are sent with their `thinking` and `redacted_thinking` blocks removed (on the request copy only; the DB is untouched).
- Messages of the **current Exchange** are sent as stored, thinking included, so a future tool loop can hand a model its own reasoning back within the Exchange.
- An earlier assistant Message whose content is only thinking cannot exist in a completed Exchange (it would have failed `EmptyResponse`), so stripping never leaves an empty message. If it somehow does, drop that message from the request rather than send empty content (*my call*).
- The cache marker goes on the last block of the last message **after** stripping.

## Failures (user's decision: no new key)

No new failure key; `Run::outcome` is unchanged. An answer with thinking but no text fails as `EmptyResponse`, whatever the stop reason (including `max_tokens`), and its thinking stays as the Partial message, now shown. A `ReasoningExhausted` key was considered and dropped.

## Known gap (user's decision: build as is)

openrouter-rs 0.22's `AnthropicContentPart::Thinking` requires `signature`. A provider that starts a thinking block without one makes the frame unparseable; the stream yields an error and the Exchange fails as `ProviderError` (`unparseable_response`). 0.23.0 has the same type. **Build as is; no fork, no patch, no workaround.** The live contract test shows whether it bites with real models. If it does, that is reported, not fixed in this increment.

## Testing

`CLAUDE.md` rules apply in full. After changes: `cargo test --workspace`, `cargo clippy --workspace --all-targets` (no new warnings), and the OpenRouter contract test. No SQL changes, so the Postgres suite is not required beyond running `postgres_chat` once if `exchange.rs` changes touch persistence.

Unit tests (in the file under test):
- `integrations` client: request built with each reasoning setting (absent → disabled; each `effort`) — assert on the serialized request JSON.
- Config: each `effort` value parses; an unknown value or a missing `effort` is rejected.
- Accumulator: thinking deltas return display deltas with their own block number; thinking then text numbers 0 then 1; empty thinking gets no number.
- `display_blocks`: thinking included; numbering matches the stream (extend the existing reload test).
- Registry: replay keeps block kinds (thinking replayed as thinking).
- `build_request`: earlier Exchanges' thinking stripped, current Exchange's kept, cache marker on the last block after stripping.
- `run_exchange`: `thinking_delta` events written then forwarded; the existing thinking-only `EmptyResponse` test now also sees the `thinking_delta` before `exchange_failed`.
- `views`: `ExchangeEvent::ThinkingDelta` serializes as `{"block", "text"}` with name `thinking_delta`; `DisplayBlock::Thinking` serializes with `"type": "thinking"`.

Live contract test (`integrations/tests/openrouter_contract.rs`, `#[ignore = "requires OpenRouter key"]`, `openrouter/free`):
- One new test: a short prompt with `effort` set streams to a stop reason without the provider rejecting the request. Assertions must not require thinking to appear (the free router picks a random model). If thinking does appear, it must have arrived as `ThinkingDelta` events.
- If the known gap bites, the test simply fails (*user's decision: "if it fails it fails"*). It keeps its single `"requires OpenRouter key"` ignore reason, does not accept `unparseable_response` as an outcome, and gets no Known bugs row; the failure is reported in the PR. It may be flaky, since only some free models send thinking without a signature.

## Left to the builder

Module layout, field and type names (e.g. how the registry stores block kinds), the exact `debug_message` wording, test names within the `CLAUDE.md` convention.

## Calls I made (check these)

- Display and stripping are unconditional; the config only shapes the request. Past Exchanges with stored thinking will now show it, and their block numbering shifts.
- SSE event name `thinking_delta` with payload `{block, text}`; display block `{"type": "thinking", "thinking": ...}`.
- Thinking blocks are numbered and shown in provider order, interleaved with text in the one block list.
- `redacted_thinking` stays ignored.
- Stripping covers `thinking` and `redacted_thinking`; current Exchange keeps its thinking.
- Stay on openrouter-rs 0.22 (0.23 doesn't fix the gap and is breaking).
