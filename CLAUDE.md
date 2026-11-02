# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Run the API server
cargo run -- serve

# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p services

# Lint
cargo clippy

# Database migrations
cargo run -- migrate up          # Apply all pending migrations
cargo run -- migrate up 5        # Apply next 5 migrations
cargo run -- migrate down 1      # Rollback last migration
cargo run -- migrate down 3      # Rollback last 3 migrations
cargo run -- migrate down        # Rollback ALL migrations (no count means all, same as reset)
cargo run -- migrate fresh       # Drop all tables and reapply from scratch
cargo run -- migrate refresh     # Rollback all, then reapply all
cargo run -- migrate reset       # Rollback all migrations
cargo run -- migrate status      # Show pending/applied status

# Entities regeneration after migration
sea-orm-cli generate entity -u <connection_url> -o entities/src/entities
```

The API listens on `127.0.0.1:3000`.

## Configuration

`config.yaml` (root) is the sole config source — no `.env`. Required fields:

```yaml
jwt:
  secret: <signing secret>
db:
  connection_url: postgres://postgres:postgres@localhost/mia_development
  schema: public
tmdb:
  authorization_token: <TMDB bearer token>
openrouter:
  api_key: <OpenRouter API key>
  model: openrouter/free
  max_tokens: 16384   # optional, default 16384
  reasoning:          # optional; absent disables reasoning
    effort: medium        # low | medium | high | xhigh | max
  data_policy: deny     # optional, default deny: allow (any provider) | deny (no prompt collection) | zdr (zero retention); free models need allow
assistant:            # optional; each limit defaults as shown and must be at least 1
  mia_search_limit: 30    # results of one mia_search call
  mia_details_limit: 60   # ids of one mia_details call
  max_tool_rounds: 8      # rounds of tool calls in one Exchange
logging:
  level: debug        # trace | debug | info | warning | error
media:
  unset_title: "Unknown"
images:
  store_path: /path/to/images
```

## Architecture

This is a Rust workspace where each crate is a strict layer — dependencies only flow downward.

```
src/main.rs          CLI entrypoint (parses "serve" / "migrate" subcommands)
api/                 HTTP layer: Axum routing, middleware, endpoint handlers
services/            Business logic and all SeaORM database queries
entities/            SeaORM entity definitions (auto-generated from DB schema)
views/               Response DTOs (Serde serialization)
transpiler/          Transpilation of search queries into SeaORM queries
integrations/        External API clients (TMDB, OpenRouter)
infrastructure/      Config loading, error types, shared initialisation
migrations/          SeaORM migration files
```

### Image handling

Images are downloaded from TMDB, resized to multiple fixed dimensions, and stored on the local filesystem under `config.images.store_path`.

## Testing

```bash
cargo test --workspace                              # unit tests only; touch nothing outside the process
cargo test -p services --no-fail-fast --tests -- --ignored   # Postgres (needs Docker) and filesystem tests; known-bug tests fail by design
cargo test -p services --test fs_image_store -- --ignored   # filesystem only (temp dir)
cargo test -p integrations -- --ignored             # TMDB and OpenRouter contract tests (live; needs config.yaml)
cargo test -p integrations --test openrouter_contract -- --ignored  # OpenRouter contract test (live, free model; needs config.yaml)
cargo clippy --workspace --all-targets              # must add no new warnings
```

After changing code, run `cargo test --workspace` and clippy. Run the relevant `--ignored` suite when you touch SQL, transactions, migrations, the TMDB client or the image store.

### Dependencies and fakes

`services` reaches the outside world only through six traits, passed as `&dyn` parameters (`Arc<dyn …>` in `AppState` and `JobDeps`). Each has a real implementation and a fake in `services::test_support` (gated by the `test-support` feature, which `services` enables for its own tests):

| Trait | Real | Fake |
|-------|------|------|
| `integrations::tmdb::Tmdb` | `TmdbClient` | `MockTmdb` (mockall: `.expect_movie_details().withf(..).returning(..)`; a method without an expectation panics) |
| `integrations::openrouter::Llm` | `OpenRouterClient` | `MockLlm` (mockall; builders in `test_support::llm`) |
| `ImageStore` | `FsImageStore` | `InMemoryImageStore` (`insert`, `files`, `get`) |
| `Jobs` (`services::jobs`) | `TokioJobs` | `RecordingJobs` (`take()` returns the recorded `Job`s), `InlineJobs` (runs jobs inline; used with real Postgres) |
| `Clock` | `SystemClock` | `FixedClock` (`new`, `set`, `advance`), `fixed_clock(y, m, d, h, min, s)` |
| `Random` | `OsRandom` | `SeqRandom` (`new()` yields `Uuid::from_u128(1000 + n)`; `with_uuids`) |

`test_support` also has: `init()` (call first in every test; fixed config, no `config.yaml`), `db` (`mock()`, `log`, `assert_statement_count`, `assert_wrote`, `string_value_written`, `assert_no_statement_starting_with`, `deleted`), `fixtures` (model builders such as `media_model`, `movie_model`, `image_model`), `errors` (`violations`, `assert_single_violation`), `time::date`, and `tmdb` builders (`movie_details`, `series_details`, `season_details`, `genre_list`, `property_changes`, `png_bytes`, …). Add new builders to `fixtures`, never locally in a test module. `SrvErr` has no `Debug`; use `.ok().expect(..)` or `matches!`.

### Rules for services code

- Parameter order: the function's own parameters ending with `db`, then `llm, registry, tmdb, store, jobs, clock, random`. A function takes only what it or a callee needs. Where clippy's `too_many_arguments` fires, add `#[allow(clippy::too_many_arguments)]`; do not bundle the dependencies into a struct.
- No direct `Utc::now`, `Uuid::new_v4`, `OsRng`, `tokio::spawn`/`task::spawn`, `tokio::fs` or `std::fs` in `services/src`. Use the traits. The exceptions are the real implementations (`SystemClock`, `OsRandom`, `FsImageStore`, `TokioJobs`) and the CLI entry points `run_refresh` and `run_linearize_ratings`.
- Background work (image resizing, and `Job::RunExchange` for chat Exchanges) goes through `Jobs`. In tests, run with `RecordingJobs`, assert the recorded jobs (`test_support::jobs::sizes`), and if needed run each one sequentially with `services::jobs::run(job, &test_support::jobs::job_deps(db, store))`. Never construct `TokioJobs` in a test. `JobDeps` holds `db`, `store`, `llm`, `registry`, `tmdb` and `clock`. `job_deps` holds a `MockLlm` and a `MockTmdb` without expectations, so a test that runs a `RunExchange` builds its own `JobDeps` with an expecting `MockLlm` (as `services/tests/postgres_chat.rs` does).

### Rules for tests

- Unit tests go in `#[cfg(test)] mod tests` at the bottom of the file under test. Real integration tests go in `services/tests/` (Postgres, filesystem) and `integrations/tests/` (TMDB, OpenRouter).
- Names are `<function>_<expected outcome>_when_<condition>`, e.g. `create_returns_existing_id_when_movie_already_added`.
- Anything that touches the network, Docker, the filesystem, `config.yaml`, env vars or the wall clock is `#[ignore = "<reason>"]` with exactly one of: `"requires TMDB token"`, `"requires OpenRouter key"`, `"requires docker"`, `"touches filesystem"`. The only other ignore reason is `"known bug: <one line>"`.
- Mock DB: queue only the results the code path consumes, in the order the queries run (futures in `try_join!` run in source order). Assert returned values first, and written values (`assert_wrote`) only when the write is the behaviour. Never assert full SQL strings. Early-return tests assert `assert_statement_count`.
- Postgres inserts use `RETURNING`, so queue a query result, not an exec result.
- `InMemoryImageStore::delete_directory` errors on a directory with no files; seed at least one file for any image a test deletes.
- `MockDatabase` cannot detect transaction-scope bugs: it logs every statement after `BEGIN` as inside the transaction, even when the code wrote through `db` instead of `tran`. Test those against real Postgres, using a trigger to inject a failure (see `services/tests/postgres_transactions.rs`).
- Postgres tests need Docker (testcontainers). Each `services/tests/postgres_*.rs` binary starts one container, migrates a template database once, and clones it per test via `common::postgres::database()`. Containers are removed when the test process exits. Use `--no-fail-fast` with `--ignored`, or cargo stops at the first binary whose known-bug tests fail.

### Known bugs

If a characterization test shows the code is wrong, do not fix the code and do not weaken the test. Keep the test asserting the correct behaviour, mark it `#[ignore = "known bug: <one line>"]`, and add a row (function, problem, test name) to the Known bugs table at the end of `docs/superpowers/specs/2026-10-29-testing-infrastructure-design.md`. Those tests fail under `--ignored` by design. Bugs a mock test cannot see get a row with a real-Postgres test instead.
