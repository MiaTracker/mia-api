# Testing infrastructure — design

Date: 2026-10-29
Branch: `testing-infrastructure` (from `master`; the OpenRouter/LLM chat work is not on this branch and will adopt this infrastructure afterwards)

## Intent

Agents working on this codebase need to validate their own changes. The test suite is that feedback loop: an agent changes something, runs `cargo test`, and gets a fast, trustworthy, deterministic answer that points at what broke.

Today only pure functions can be tested: `services/src/services/ratings_linearization.rs` has three unit tests for the linearization algorithm (`test_uniform_six_ratings`, `test_uneven_multiplicities`, `test_infeasible_fallback`). They pass and stay as they are. Anything that touches the DB, TMDB, the filesystem, time or randomness has no way to be tested, and that is what this work adds.

## Goals

- **Broad unit coverage of the `services` crate** (every public service function, plus the job handlers), using SeaORM's `MockDatabase` for the DB and mocks/fakes for every other side effect.
- **Every external side effect and source of non-determinism behind an injectable interface:** TMDB, the image filesystem, background work, the current time, and randomness (UUIDs, password salts).
- **A small set of real integration tests** that each catch something mocks structurally cannot:
  1. **TMDB contract**: real API responses still deserialize into our views; URLs, auth and pagination are correct.
  2. **Postgres**: real SQL runs against the real schema (catches invalid queries, entity/migration drift, Postgres-specific functions), via `testcontainers`.
  3. **Real `ImageStore`**: directory layout, file naming, create-new semantics and cleanup on a real filesystem (temp dir). Ignored by default like the others.
- **One home for test infrastructure** that later work (the OpenRouter chat feature first) reuses.

## Non-goals

- Changing what the application does. The refactor is about injecting dependencies; production behaviour stays the same except where noted below.
- Coverage targets or metrics tooling.
- CI configuration.

## Anti-goals (would make this a failure even if it technically works)

- **Non-deterministic tests.** A test that passes or fails depending on scheduling is worse than no test, because an agent can't tell a real regression from noise. **No spawned or multi-threaded work in unit tests.** Concurrency within a single task (`try_join!`, `buffer_unordered`, as in `refresh` and `statistics`) is fine and needs no change: mocked calls complete on their first poll, so the order is the same every run.
- **A default `cargo test` that touches anything outside the process.** The non-ignored tests are completely independent of the environment: no Postgres, Docker, TMDB token, network, filesystem (not even a temp dir), `config.yaml`, environment variables or wall-clock time. Everything that touches the outside world is `#[ignore]`d.
- **Hidden global swapping.** Mocks are passed in explicitly. No global or task-local "current client" that tests replace.
- **Tests that exist without catching anything.** Real integration tests are only there for what mocks can't see.

## Decisions (made by the user)

### 1. TMDB behind a trait, passed explicitly

- `integrations` gets a `Tmdb` trait covering every operation in `integrations::tmdb::services::*` (movies, series, seasons, episodes, changes, property changes, alternative titles, images metadata, image bytes, image stream, search, genres, languages, configuration).
- The real implementation is a client struct created once at startup.
- **Only service functions that need TMDB take a `tmdb` parameter** alongside `db`; the others keep their signatures. (A context struct bundling everything and a swappable global were both considered and rejected.)
- The instance lives in `AppState` next to `conn` and is passed down from handlers.
- Future integrations (OpenRouter) follow the same pattern: an `llm` parameter on the functions that need it.

### 2. Image filesystem behind a trait (`ImageStore`)

- All filesystem access in `services/src/services/images.rs` goes through an `ImageStore` trait: serving a stored file, writing a new file (create-new semantics, clean up on failed write), and deleting an image's directory.
- The real implementation is rooted at `config.images.store_path`. The test implementation is an in-memory fake.
- Passed explicitly like `tmdb`.

### 3. Background work as typed jobs (`Jobs`)

Background work stops being a direct `tokio::spawn` and becomes a value submitted through a `Jobs` trait, passed explicitly like `tmdb`.

- `Job` is an enum. Each variant carries the owned data its handler needs.
- `Jobs::submit(&self, job: Job)` is the only way service code starts background work.
- There are three implementations:
  - **`TokioJobs`** (production) runs each submitted job with `tokio::spawn` and logs failures. It is constructed at startup holding what handlers need (DB connection, `ImageStore`, …).
  - **`RecordingJobs`** (unit tests) stores submitted jobs without running them; the test can take them out and inspect them.
  - **`InlineJobs`** (Postgres integration tests) runs each job to completion at the point of submission, so a whole flow runs for real and deterministically.
- **Unit-test pattern:** call the function with `RecordingJobs`, assert the exact jobs it submitted, then run each recorded job's handler **one at a time** against the mock DB and fakes, and assert their effects. No unit test ever has two things touching a mock concurrently.
- **A handler that fans out does so by submitting jobs**, never by spawning or by `async_scoped`. The same record-then-run-sequentially pattern applies at every level.

### 4. Image saving is flattened to one job level

Today: `save_tmdb_image` saves the first (original) size synchronously, then `tokio::spawn`s `save_image_sizes_task`, which fans out with `async_scoped::TokioScope` over all remaining sizes, each borrowing the decoded image.

New shape:

- `save_tmdb_image` saves the first size synchronously as today, computes the remaining target sizes itself (pure math, same algorithm as `save_image_sizes_task` + `calculate_image_size`), and submits one `Job::SaveImageSize { image_id, width, height, directory, extension, format, image }` per size.
- The `SaveImageSize` handler resizes once, writes through `ImageStore`, and inserts one `image_sizes` row. It is the only leaf.
- `save_image_sizes_task` and the `async_scoped` usage (with its `unsafe` block) are removed. Per-size error logging moves to `TokioJobs`.
- **The decoded image is shared as `Arc<DynamicImage>`.** Jobs must own their data (they outlive the submitting stack frame), and `Arc` keeps today's no-copy property.

The resulting production behaviour change is small and accepted: sizes beyond the first are saved by independent spawned tasks rather than one task that joins them.

### 5. Postgres integration tests use `testcontainers`

- Postgres via `testcontainers` (Postgres module). The tests start a container, apply the real migrations from the `migrations` crate, and run service flows against it with real `TokioJobs` swapped for `InlineJobs`.
- These need Docker, so they're `#[ignore = "requires docker"]`.
- **They need only Docker: no network, no TMDB token** (my call). Today three data migrations (`m20251017_120428_images`, `m20260417_000001_episodes`, `m20260417_000003_season_episode_images`) call TMDB `initialize()` unconditionally, even on an empty database. After the refactor, the real TMDB client fetches the TMDB configuration lazily on first use, so a migration that has no rows to process never contacts TMDB. Fixed fake values serve the remaining fields.
- **One container per test binary, one database per test** (my call). `m20260417_000002_add_still_image_type` opens its own connection from the global `config().db.connection_url`, because SeaORM wraps Postgres migrations in a transaction. So the bootstrap starts the container first, then initializes the global config with the container's URL (each file in `services/tests/` is its own binary, so this doesn't clash with the unit-test config). It runs migrations once into a template database. Each test then gets a fresh `CREATE DATABASE … TEMPLATE …` copy, giving isolated, already-migrated databases without re-running migrations. That migration's connection handling is left as it is.

### 6. Broad unit coverage now

This pass covers every public function in `services` (and the job handlers), not just the infrastructure plus samples.

### 7. Existing bugs found by tests: correct test, ignored, listed

When writing a test exposes a bug in existing code:

- Write the test for the **correct** behaviour, not today's.
- Mark it `#[ignore = "known bug: <one-line description>"]`.
- Don't fix the bug in this work.
- Add it to the **Known bugs** list at the end of this document: the function, what's wrong, and the name of the ignored test.

Fixing a bug later then means fixing the code and removing the `ignore`.

### 8. Time and randomness behind traits (`Clock`, `Random`)

Passed explicitly to the functions that need them, like `db`, `tmdb` and `jobs`.

- **`Clock`** returns the current UTC time. The real implementation uses `Utc::now()`. The test implementation returns a fixed time the test sets (and can advance).
- **`Random`** returns new UUIDs and password salts. The real implementation uses `Uuid::new_v4()` and `OsRng`. The test implementation is deterministic: a sequence of UUIDs the test can predict or preset (so collision paths, such as the image-directory conflict loop, are testable), and a fixed salt.
- Current call sites that switch over:
  - `Utc::now()`: `refresh::refresh`, `users` (token issuing), `watchlist`, `app_tokens`, and `api/src/middleware/auth.rs` (token expiry check; `AppState` holds the `Clock`).
  - `Uuid::new_v4()`: the image directory name in `images::save_tmdb_image`, `app_tokens`, and the user UUID in `view_trait_implementations.rs`.
  - `SaltString::generate(&mut OsRng)`: both places in `users`.
- **Conversions don't read the clock or generate IDs** (my call). The `IntoActiveModel` implementations that set `date_added` from `Utc::now()` (in `integrations_trait_implementations.rs`) and the one that sets a UUID (in `view_trait_implementations.rs`) leave those fields to the calling service, which sets them from `Clock`/`Random`. This keeps the conversion traits' signatures unchanged.
- Any new code that needs time or randomness uses these traits. Direct `Utc::now()`, `Uuid::new_v4()` or `OsRng` in `services` is a review failure.

## Layout

Dependencies still only flow downward (`api → services → integrations/entities/…`).

- `integrations`: the `Tmdb` trait and the real client. The client owns its configuration (bearer token, image base URL from the TMDB configuration endpoint) instead of reading globals. This also fixes the current `pub const CLIENT: Lazy<reqwest::Client>`, which, being a `const`, builds a new client on every use.
- `services/src/jobs.rs`: the `Job` enum, the `Jobs` trait, `TokioJobs`, and a single `run(job, deps)` dispatch that `match`es each variant to its handler.
- Job handlers live with the code they came from (e.g. the `SaveImageSize` handler in `services/src/services/images.rs`).
- `ImageStore` trait and real implementation: in `services` (my call: it's only used by the image service; the builder may move it if a better home emerges).
- **Test support: a module in `services` behind a `test-support` cargo feature**, not `#[cfg(test)]`, because `#[cfg(test)]` code is invisible to integration tests in `services/tests/` and to other crates. It holds:
  - the `Tmdb` mock,
  - the in-memory `ImageStore`,
  - `RecordingJobs` and `InlineJobs`,
  - the fixed `Clock` and deterministic `Random`,
  - a one-time initializer for the global `config()` and `tmdb_config()` with fixed test values,
  - helpers/builders for `MockDatabase` results and common entity models,
  The testcontainers Postgres bootstrap (start container, run migrations, return a `DbConn`) is not part of this module: it lives in `services/tests/common/postgres.rs`, so that `testcontainers` and `migrations` stay dev-only dependencies of `services` (deliberate deviation, reviewed).
  `services` enables it for its own tests through a dev-dependency on itself with the feature. The OpenRouter work will add its mock here.
- `Clock` and `Random` traits and their real implementations: in `services` (my call), next to `Jobs`.
- `api`: `AppState` gains the `Tmdb`, `ImageStore`, `Jobs`, `Clock` and `Random` instances. `launch()` constructs the real ones. The refresh scheduler in `api/src/lib.rs` keeps calling `refresh()`, which is tested directly.
- `migrations`: the data migrations that call TMDB switch from the free functions to the real `TmdbClient`, built from config. They keep their own filesystem code and do not use `ImageStore`: it lives in `services`, and `migrations` must not depend on `services` (deliberate deviation, reviewed).

## How tests run

(The ignore reasons, the test file locations and the `CLAUDE.md` section are my calls.)

- `cargo test` (default): unit tests only. They touch nothing outside the process.
- `cargo test -- --ignored`: every real integration test:
  - TMDB contract tests: `#[ignore = "requires TMDB token"]`, token read from `config.yaml`.
  - Postgres tests: `#[ignore = "requires docker"]`.
  - Real `ImageStore` tests: `#[ignore = "touches filesystem"]`, using a temp dir.
- `CLAUDE.md` gets a Testing section with these commands and the conventions below, so future agents find them.

## Conventions

(My calls.)

- **Test names state function and scenario**, e.g. `create_returns_existing_id_when_movie_already_added`, so a failure says what broke without opening the file.
- **Each test queues only the mock results its path needs**, built with shared helpers, so a harmless query change breaks as few tests as possible. Where mock-DB statement order is genuinely irrelevant, assert on the set of statements rather than their sequence.
- **Mocks are explicit and per-test.** No shared mutable mock state across tests.
- Unit tests live next to the code (`#[cfg(test)] mod tests`). Real integration tests live in `services/tests/` (Postgres, ImageStore) and `integrations/tests/` (TMDB).

## Real integration tests: what each must cover

- **TMDB contract:** one test per `Tmdb` method against stable, well-known TMDB IDs, asserting the response deserializes and key fields are populated. Pagination of the changes endpoints is exercised at least once.
- **Postgres:** migrations apply to an empty database. Then representative flows run against real SQL, prioritising the ones with the most hand-built or Postgres-specific SQL: media search (the `transpiler` output), statistics, ratings linearization, refresh, movie and series creation (with seasons/episodes and images via `InlineJobs`, a fake `Tmdb`, the in-memory `ImageStore`, and the fixed `Clock`/deterministic `Random`), logs/watchlist/tags/sources CRUD. The builder picks the exact set.
- **ImageStore:** write/read/delete round trip, create-new refuses to overwrite, a failed write leaves no partial file (`services/tests/fs_image_store_partial_write.rs` and `fs_image_store_partial_flush.rs`, which lower `RLIMIT_FSIZE` and so need one test per binary and Linux), directory deletion.

## Left to the builder

- `mockall` vs hand-written fakes for each trait (likely `mockall` for `Tmdb`, hand-written for `ImageStore` and the jobs implementations).
- How the traits are made object-safe for async (e.g. `async-trait`) and whether they're used as `Arc<dyn …>` or generics. The deciding factor is readability at call sites and in tests.
- The exact `ImageStore` method set and the streaming type returned for served images.
- Order of work. Suggested: traits and test support first, then refactor services module by module with their tests, then the real integration tests, then `CLAUDE.md`.

## Known bugs

Found while designing or writing tests. Each has an ignored test asserting the correct behaviour where one is possible; rows without one say why.

| Function | Problem | Ignored test |
|----------|---------|--------------|

The bugs listed here earlier were fixed on 2026-11-04, and their tests now run as ordinary unit tests or under their `requires docker` / `touches filesystem` ignore reasons.

### Not bugs

| Function | Behaviour | Test |
|----------|-----------|------|
| `transpiler::transpile` (filter part of a search query) | `and` binds looser than `or`: `Parser::logic_and` parses its right side with `equality()` (the whole `or` level), so `a and b or c` means `a and (b or c)`. The filter language was designed this way. | `parse_binds_and_looser_than_or_when_filter_mixes_them` (`transpiler/src/parser.rs`) |
