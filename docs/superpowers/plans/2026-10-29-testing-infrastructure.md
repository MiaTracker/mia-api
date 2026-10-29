# Testing Infrastructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make every service function unit-testable with no environment (mock DB, mock TMDB, in-memory image store, recorded jobs, fixed clock, deterministic randomness), cover every public service function, and add ignored real-integration tests for TMDB, Postgres and the filesystem.

**Architecture:** Each external side effect gets a trait that is passed explicitly to the functions that need it (`tmdb: &dyn Tmdb`, `store: &dyn ImageStore`, `jobs: &dyn Jobs`, `clock: &dyn Clock`, `random: &dyn Random`). Production instances live in `api::AppState`. Test fakes live in `services::test_support`. Background work becomes typed `Job` values that tests record and then run one at a time.

**Tech Stack:** Rust 2024 workspace, SeaORM 1.1 (`mock` feature), `async-trait`, `mockall`, `testcontainers` + `testcontainers-modules` (postgres), `tempfile`, `image`.

**Spec:** `docs/superpowers/specs/2026-10-29-testing-infrastructure-design.md`

## Global Constraints

- **The default `cargo test` touches nothing outside the process:** no network, Docker, filesystem (not even a temp dir), `config.yaml`, env vars or wall-clock time. Anything that does is `#[ignore = "<reason>"]`, using exactly one of these reasons: `"requires TMDB token"`, `"requires docker"`, `"touches filesystem"`.
- **No spawned or multi-threaded work in unit tests.** Concurrency within a single task (`try_join!`, `buffer_unordered`) is fine.
- **Dependency parameter order:** first the function's existing parameters (ending with `db`), then `tmdb`, `store`, `jobs`, `clock`, `random`, in that order. A function only gets the parameters it (or a callee) actually needs.
- **Parameter types:** `&dyn Tmdb`, `&dyn ImageStore`, `&dyn Jobs`, `&dyn Clock`, `&dyn Random`. `AppState` and job deps hold them as `Arc<dyn …>`. Where clippy's `too_many_arguments` fires because of these parameters, add `#[allow(clippy::too_many_arguments)]` on that function. Do not bundle parameters into a struct; that option was rejected.
- **Async traits use `#[async_trait::async_trait]`** and are `Send + Sync`.
- **Direct `Utc::now()`, `Uuid::new_v4()`, `OsRng`, `tokio::spawn`, `task::spawn`, `tokio::fs` or `std::fs` in `services/src` is forbidden** after Task 7, except inside the real trait implementations (`SystemClock`, `OsRandom`, `FsImageStore`, `TokioJobs`) and the CLI entry points `run_refresh` and `run_linearize_ratings`.
- **Test names** say the function and the scenario: `<function>_<expected outcome>_when_<condition>`, e.g. `create_returns_existing_id_when_movie_already_added`. The three existing tests in `ratings_linearization.rs` keep their names.
- **Unit tests** go in `#[cfg(test)] mod tests` at the bottom of the file under test. Real integration tests go in `services/tests/` and `integrations/tests/`.
- **Characterization tests for existing code are expected to pass on the first run.** If one fails, decide whether the test or the code is wrong. If the code is wrong, apply the **known-bug rule**: keep the test asserting the *correct* behaviour, add `#[ignore = "known bug: <one line>"]`, don't fix the code, and add a row to the **Known bugs** table at the end of the spec (function, problem, test name). (This is the one allowed ignore reason besides the three above.)
- **Mock DB results:** queue only what the code path consumes. Assert on returned values first, and on written values (via the transaction log helpers in Task 1) only where the write *is* the behaviour. Never assert full generated SQL strings.
- **Commits** end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Run `cargo clippy --workspace --all-targets`** before every commit. No new warnings.

## Review Focus

1. **Mock-DB queue exhaustion or leftovers.** A test that queues more results than the path consumes hides an early return; one that queues fewer fails with an exhaustion error that's hard to read. Every test that exercises an early-return path (not found, already exists, rule violation) asserts the transaction log has exactly the expected number of statements (`assert_statement_count`, Task 1).
2. **Ownership checks across users.** Every `media_id`/`log_id`/`source_id`-taking function filters by `user.id`. Each module's tests include one `..._returns_not_found_when_media_belongs_to_another_user` case that verifies the filter is applied (Tasks 2, 8, 9, 10).
3. **`save_tmdb_image` on edge-size images.** Images whose short side is ≤ 300 px produce *no* `SaveImageSize` jobs (the loop condition is `threshold > short - 200`). Images with a short side < 200 px would underflow `short - 200` (`u32`). Task 6 pins both: no jobs for 300×450, and for 150×150 it must not panic (if it does, apply the known-bug rule).
4. **Expired and future-dated tokens.** The auth middleware compares `exp` against `Clock`. Task 3 tests `login` with a fixed clock and asserts `exp == now + 60 days` and `iat == now` exactly.
5. **TMDB returning non-200.** Services convert `integrations::infrastructure::Error` to `SrvErr` (`SrvErr::Integration`/`From`). Task 8 covers `movies::create` returning `Err` when `movie_details` fails, and asserts nothing is written to the DB.

---

## File Structure

**Create**
- `services/src/test_support/mod.rs` — `init()`, re-exports.
- `services/src/test_support/config.rs` — fixed global config + TMDB config.
- `services/src/test_support/db.rs` — `MockDatabase` helpers and transaction-log assertions.
- `services/src/test_support/fixtures.rs` — entity/view builders (`user()`, `media_model()`, …).
- `services/src/test_support/tmdb.rs` — `MockTmdb` (`mockall::mock!`) and TMDB view builders.
- `services/src/test_support/image_store.rs` — `InMemoryImageStore`.
- `services/src/test_support/jobs.rs` — `RecordingJobs`, `InlineJobs`.
- `services/src/test_support/time.rs` — `FixedClock`, `SeqRandom`.
- `services/src/infrastructure/clock.rs` — `Clock`, `SystemClock`.
- `services/src/infrastructure/random.rs` — `Random`, `OsRandom`.
- `services/src/infrastructure/image_store.rs` — `ImageStore`, `FsImageStore`.
- `services/src/jobs.rs` — `Job`, `Jobs`, `JobDeps`, `run`, `TokioJobs`.
- `integrations/src/tmdb/client.rs` — `Tmdb` trait, `TmdbClient`.
- `integrations/tests/tmdb_contract.rs` — ignored real-TMDB tests.
- `services/tests/common/mod.rs`, `services/tests/common/postgres.rs` — testcontainers bootstrap.
- `services/tests/postgres_*.rs` — ignored Postgres flow tests (one file per area, listed in Task 12).
- `services/tests/fs_image_store.rs` — ignored real-filesystem tests.

**Modify**
- `infrastructure/src/config.rs` — `init_config_once`.
- `services/src/infrastructure/constants.rs` — `init_tmdb_config_once`.
- `services/src/infrastructure/initialize.rs` — takes `&dyn Tmdb`.
- `services/src/services/*.rs` — dependency parameters and `#[cfg(test)] mod tests`.
- `services/src/infrastructure/integrations_trait_implementations.rs`, `view_trait_implementations.rs` — stop reading the clock and generating UUIDs.
- `integrations/src/tmdb/services/*`, `integrations/src/infrastructure/http.rs`, `integrations/src/constants.rs` — free functions and globals replaced by `TmdbClient`.
- `integrations/src/tmdb/views/*.rs` — add `Default` (and `Clone`, `Debug` where missing) derives.
- `migrations/src/m20251017_120428_images.rs`, `m20260417_000001_episodes.rs`, `m20260417_000003_season_episode_images.rs` — use `TmdbClient`.
- `api/src/infrastructure/app_state.rs`, `api/src/lib.rs`, `api/src/endpoints/*.rs`, `api/src/middleware/auth.rs` — hold and pass dependencies.
- `src/main.rs` — no change expected. `run_refresh` builds its own deps.
- `CLAUDE.md` — Testing section.

---

### Task 1: Test-support foundation, config hooks, DB-only reads

**Files:**
- Modify: `services/Cargo.toml`, `infrastructure/src/config.rs`, `services/src/infrastructure/constants.rs`, `services/src/lib.rs`
- Create: `services/src/test_support/{mod.rs,config.rs,db.rs,fixtures.rs}`
- Test: `services/src/services/languages.rs`, `services/src/services/genres.rs`, `services/src/services/users.rs` (only `profile`, `query_user_by_uuid`, `query_user_by_app_token`, `index`)

**Interfaces:**
- Produces:
  - `infrastructure::init_config_once(f: impl FnOnce() -> Configuration) -> Arc<Configuration>` (uses `CONFIG.get_or_init`; the existing `load_config` path keeps its "set twice" failure).
  - `services::infrastructure::constants::init_tmdb_config_once(config: &integrations::tmdb::views::Configuration)` (`get_or_init` on `TMDB_CONFIGURATION`, same parsing as `set_tmdb_config`).
  - `services::test_support::init()`: idempotent, calls both with `test_config()` / `test_tmdb_configuration()`.
  - `test_support::config::test_config() -> Configuration`: jwt secret `"test-secret"`, db url `"postgres://unused"`, schema `"public"`, tmdb token `"test-token"`, refresh_interval `None`, logging `Debug`, unset_title `"Unknown"`, store_path `"/nonexistent-test-store"`.
  - `test_support::config::test_tmdb_configuration() -> integrations::tmdb::views::Configuration`: base urls `"http://image.tmdb.test/t/p/"` / `"https://image.tmdb.test/t/p/"`. Backdrop sizes `["w300","w780","w1280","original"]`, poster `["w92","w154","w185","w342","w500","w780","original"]`, still `["w92","w185","w300","original"]`, logo `["w45","w92","w154","w185","w300","w500","original"]`, profile `["w45","w185","h632","original"]`.
  - `test_support::db::mock() -> MockDatabase` (Postgres backend).
  - `test_support::db::log(db: DatabaseConnection) -> Vec<Statement>` (flattens `into_transaction_log()`).
  - `test_support::db::assert_statement_count(log: &[Statement], n: usize)`.
  - `test_support::db::assert_wrote(log: &[Statement], sql_prefix: &str, expected_values: &[sea_orm::Value])`: passes if some statement's SQL starts with `sql_prefix` (e.g. `r#"INSERT INTO "tags""#) and its values contain every expected value. The failure message prints all logged statements.
  - `test_support::fixtures::user() -> CurrentUser` (id `1`, uuid `Uuid::from_u128(1)`, username `"tester"`, email `"tester@example.test"`, admin `false`, `though_bot: false`). Also `admin()`, and model builders for every entity used by Tasks 2–11 (`media_model(id, user_id, MediaType)`, `movie_model(id)`, `series_model(id)`, `image_model(id)`, …), each returning a fully populated `Model` that tests modify with struct-update syntax.

- [ ] **Step 1: Add dependencies and the feature**

`services/Cargo.toml`:
```toml
[dependencies]
async-trait = "0.1"
mockall = { version = "0.13", optional = true }

[features]
test-support = ["dep:mockall", "sea-orm/mock"]

[dev-dependencies]
services = { path = ".", features = ["test-support"] }
mockall = "0.13"
sea-orm = { version = "1.1.0", features = ["runtime-tokio-rustls", "sqlx-postgres", "mock"] }
tokio = { version = "1.47.0", features = ["macros", "rt"] }
```
In `services/src/lib.rs`: `#[cfg(any(test, feature = "test-support"))] pub mod test_support;`

- [ ] **Step 2: Write failing tests**

In `languages.rs`: `index_returns_all_languages_sorted_as_queried` (queue two `languages::Model`s, assert the returned `LanguageIndex` list matches them in order).
In `genres.rs`: `index_returns_genre_names_for_user`, `create_returns_false_when_genre_already_on_media`, `create_returns_not_found_when_media_belongs_to_another_user`, `delete_removes_media_genre_link`, `add_genre_to_media_creates_genre_when_missing`.
In `users.rs`: `profile_maps_current_user_fields`, `query_user_by_uuid_returns_none_when_missing`, `query_user_by_app_token_returns_owner`, `index_returns_all_users`.
Each test starts with `test_support::init();`, and uses `#[tokio::test]` when async.

- [ ] **Step 3: Run and confirm they fail to compile** (`test_support` missing)

Run: `cargo test -p services`
Expected: compile error on `test_support`.

- [ ] **Step 4: Implement `init_config_once`, `init_tmdb_config_once` and the `test_support` modules listed above**

- [ ] **Step 5: Run tests**

Run: `cargo test -p services`
Expected: all new tests pass, plus the 3 existing linearization tests. Any failure goes through the known-bug rule.

- [ ] **Step 6: Commit** — `Add services test-support foundation`

---

### Task 2: DB-only CRUD coverage

**Files:**
- Test: `services/src/services/{tags.rs,titles.rs,sources.rs,logs.rs}`. (Masterdata is in Task 4 because it calls TMDB.)

**Interfaces:**
- Consumes: Task 1 `test_support::{init, db::*, fixtures::*}`.

- [ ] **Step 1: Write characterization tests for every public function** in `tags`, `titles`, `sources`, `logs`. Each function gets at minimum:
  - one happy path asserting the return value and (for writes) `assert_wrote` on the main insert/update/delete;
  - `..._returns_not_found_when_media_belongs_to_another_user` (empty query result for the ownership query, `assert_statement_count`);
  - each `SrvErr::RuleViolation` branch the function has, asserting the exact `RuleViolation` variant.

  Also:
  - `logs::update_media_rating`: `update_media_rating_sets_stars_to_average_of_rated_logs` and `update_media_rating_clears_stars_when_no_rated_logs`.
  - `titles::set_primary`: `set_primary_unsets_previous_primary_title`.
  - `sources::delete_from_media`: covered directly, not only via `delete`.

- [ ] **Step 2: Run** `cargo test -p services`. Expected: pass. Failures go through the known-bug rule.

- [ ] **Step 3: Commit** — `Cover tags, titles, sources and logs services`

---

### Task 3: `Clock` and `Random`; users, app tokens, watchlist add

**Files:**
- Create: `services/src/infrastructure/clock.rs`, `services/src/infrastructure/random.rs`, `services/src/test_support/time.rs`
- Modify: `services/src/infrastructure/mod.rs`, `services/src/infrastructure/view_trait_implementations.rs:23`, `services/src/services/{users.rs,app_tokens.rs,watchlist.rs}`, `api/src/infrastructure/app_state.rs`, `api/src/lib.rs`, `api/src/middleware/auth.rs:59`, the `api/src/endpoints` handlers calling the changed functions
- Test: `users.rs`, `app_tokens.rs`, `watchlist.rs`

**Interfaces:**
- Produces:
  - `pub trait Clock: Send + Sync { fn now(&self) -> DateTime<Utc>; }`, `pub struct SystemClock;`
  - `pub trait Random: Send + Sync { fn uuid(&self) -> Uuid; fn salt(&self) -> SaltString; }`, `pub struct OsRandom;`
  - Both re-exported from `services::infrastructure`.
  - `test_support::FixedClock::new(t: DateTime<Utc>)`, `.set(t)`, `.advance(d: chrono::Duration)`.
  - `test_support::SeqRandom::new()`: `uuid()` returns preset UUIDs first (`SeqRandom::with_uuids(Vec<Uuid>)`), then `Uuid::from_u128(1000 + n)`. `salt()` always returns `SaltString::from_b64("dGVzdHNhbHR0ZXN0c2FsdA").unwrap()`.
  - New signatures:
    - `users::register(user, db, random: &dyn Random)`
    - `users::login(user, jwt_secret, db, clock: &dyn Clock)`
    - `users::change_password(pass_change, user, db, random)`
    - `app_tokens::generate(name, jwt_secret, user, db, clock, random)`
    - `watchlist::add(media_id, user, db, clock)`
  - `IntoActiveModel<users::ActiveModel> for &UserRegistration` sets `uuid: NotSet`. `register` sets it from `random.uuid()`.
  - `AppState { conn, jwt_secret, clock: Arc<dyn Clock>, random: Arc<dyn Random> }`. The auth middleware uses `state.clock.now()`.

- [ ] **Step 1: Write failing tests**
  - `register_stores_uuid_from_random_and_argon2_hash_with_fixed_salt` (assert the inserted `uuid == Uuid::from_u128(1000)` and that the hash string contains `"dGVzdHNhbHR0ZXN0c2FsdA"`)
  - `register_returns_rule_violation_when_username_taken`, and the same for each other `RuleViolation` branch
  - `login_issues_token_with_iat_now_and_exp_now_plus_60_days` (decode the JWT with `"test-secret"`, `FixedClock` at `2026-01-01T00:00:00Z`)
  - `login_returns_unauthorized_when_password_wrong`
  - `change_password_rehashes_with_fixed_salt`
  - `delete_removes_user`
  - `generate_returns_token_with_uuid_from_random_and_generated_at_now`
  - `generate_returns_rule_violation_when_name_exists`
  - `index_lists_tokens`, `revoke_deletes_token_by_name`
  - `add_sets_date_added_from_clock`, `add_returns_false_when_already_on_watchlist`, `remove_deletes_watchlist_entry`

- [ ] **Step 2: Run** `cargo test -p services` and confirm compile failure on the missing traits/arguments.

- [ ] **Step 3: Implement the traits, real impls, fakes and signature changes; update the `api` callers and `AppState` construction in `launch()` (`Arc::new(SystemClock)`, `Arc::new(OsRandom)`)**

- [ ] **Step 4: Run** `cargo test -p services && cargo build --workspace`. Expected: pass and build.

- [ ] **Step 5: Commit** — `Inject Clock and Random; cover users, app tokens, watchlist add`

---

### Task 4: `Tmdb` trait and `TmdbClient`; masterdata and configuration

**Files:**
- Create: `integrations/src/tmdb/client.rs`, `services/src/test_support/tmdb.rs`
- Modify:
  - `integrations/src/tmdb/mod.rs`, `integrations/src/tmdb/services/*` (delete the free functions, or make them private helpers behind the client), `integrations/src/infrastructure/http.rs` (no global `CLIENT`/`TMDB_HEADERS`), `integrations/src/constants.rs` (remove `TMDB_CONFIGURATION`), `integrations/src/tmdb/views/*.rs` (derive `Default`, `Clone`, `Debug` on every struct)
  - `integrations/Cargo.toml` (`async-trait`, `futures-util` already present, `tokio` `sync` feature for `OnceCell`)
  - `services/src/infrastructure/initialize.rs`
  - `services/src/services/{masterdata.rs,configuration.rs,images.rs (get_tmdb only)}`
  - The 3 migrations listed in File Structure
  - `api/src/lib.rs`, `api/src/infrastructure/app_state.rs`, `api/src/endpoints/images.rs` (`get_tmdb`), and the masterdata endpoint if one exists
- Test: `masterdata.rs`, `configuration.rs`, `images.rs` (`get_tmdb`)

**Interfaces:**
- Produces (in `integrations::tmdb`):
```rust
#[async_trait]
pub trait Tmdb: Send + Sync {
    async fn movie_details(&self, movie_id: i32) -> Result<MovieDetails, Error>;
    async fn movie_alternative_titles(&self, movie_id: i32) -> Result<MovieTitles, Error>;
    async fn movie_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error>;
    async fn movie_property_changes(&self, movie_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn movie_images(&self, movie_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error>;
    async fn series_details(&self, series_id: i32) -> Result<SeriesDetails, Error>;
    async fn series_alternative_titles(&self, series_id: i32) -> Result<SeriesTitles, Error>;
    async fn series_changed(&self, start: NaiveDate, end: NaiveDate) -> Result<Vec<i32>, Error>;
    async fn series_property_changes(&self, series_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn season_details(&self, series_id: i32, season_number: i32) -> Result<SeasonDetails, Error>;
    async fn season_property_changes(&self, season_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn episode_property_changes(&self, episode_id: i32, start: NaiveDate, end: NaiveDate) -> Result<PropertyChanges, Error>;
    async fn series_images(&self, series_id: i32, original_language: &Option<String>) -> Result<TmdbImages, Error>;
    async fn search_multi(&self, query: String) -> Result<MultiResults, Error>;
    async fn movie_genres(&self) -> Result<GenreList, Error>;
    async fn tv_genres(&self) -> Result<GenreList, Error>;
    async fn languages(&self) -> Result<Vec<Languages>, Error>;
    async fn configuration(&self) -> Result<Configuration, Error>;
    async fn image(&self, path: &str) -> Result<Bytes, Error>;
    async fn image_stream(&self, size_slug: String, path: String)
        -> Result<(StatusCode, String, Option<BoxStream<'static, Result<Bytes, Error>>>), Error>;
}
pub struct TmdbClient { /* reqwest::Client, token, base_url, OnceCell<Configuration> */ }
impl TmdbClient { pub fn new(authorization_token: String) -> Self; pub fn with_base_url(authorization_token: String, base_url: String) -> Self; }
```
  - `TmdbClient::new` makes no network calls. `image`/`image_stream` fetch and cache `configuration()` on first use (`tokio::sync::OnceCell`). URLs, query strings, retry behaviour and error mapping are unchanged from today's functions.
  - `services::infrastructure::initialize(tmdb: &dyn Tmdb)` calls `tmdb.configuration()` and `set_tmdb_config`.
  - New signatures:
    - `masterdata::refresh(db, tmdb)`
    - `images::get_tmdb(slug, path, tmdb)`
    - `configuration::images()` (unchanged: reads `tmdb_config()`)
  - `AppState` gains `tmdb: Arc<dyn Tmdb>`. `launch()` builds `TmdbClient::new(config().tmdb.authorization_token.clone())` and calls `initialize(&*tmdb)`.
  - `test_support::MockTmdb` comes from `mockall::mock!` covering the whole trait. `test_support::tmdb` builders: `movie_details(id) -> MovieDetails`, `series_details(id)`, `season_details(series_id, n)`, `tmdb_images()`, `genre_list(&[(id, name)])`, `property_changes(&[(key, …)])`, `png_bytes(width, height) -> Bytes` (encodes an in-memory `RgbImage` to PNG, no filesystem).
  - Migrations: replace `initialize::initialize().await` with `let tmdb = TmdbClient::new(config().tmdb.authorization_token.clone());`, and use `tmdb.image(path)` and `tmdb.season_details(..)`. Construct it where `initialize()` used to be called. It's safe on an empty DB because nothing is fetched until it's used. Their filesystem code stays as it is (they don't depend on `services`; spec deviation, see the handoff note).

- [ ] **Step 1: Write failing tests**
  - `configuration::images_returns_tmdb_image_configuration` (sizes from `test_tmdb_configuration`)
  - `images::get_tmdb_passes_through_status_and_content_type`, `get_tmdb_returns_err_when_tmdb_fails`
  - `masterdata::refresh_inserts_missing_languages_and_genres` (mock `languages`, `movie_genres`, `tv_genres`)
  - `masterdata::refresh_leaves_existing_genres_untouched`
  - `masterdata::refresh_writes_inside_its_transaction`: this is the known bug from the spec. Assert that every write statement in the log is inside the transaction (SeaORM's mock log records `BEGIN`/`COMMIT` as separate `Transaction` entries; use `db.into_transaction_log()` directly). Mark it `#[ignore = "known bug: masterdata::refresh opens a transaction but writes through db"]` and fill the test name into the spec's Known bugs row.

- [ ] **Step 2: Run** `cargo test -p services` and confirm the compile failure.

- [ ] **Step 3: Implement `Tmdb`, `TmdbClient`, `MockTmdb`, the builders, and the signature, `AppState` and migration changes.** Services modules still calling the free functions (movies, series, media, refresh, images::save_tmdb_image) get a `tmdb: &dyn Tmdb` parameter in this task, threaded to their callers, with no new tests yet. Their coverage comes in Tasks 8–11.

- [ ] **Step 4: Run** `cargo test -p services && cargo build --workspace && cargo clippy --workspace --all-targets`. Expected: pass, build, no new warnings.

- [ ] **Step 5: Commit** — `Put TMDB behind a Tmdb trait; cover masterdata and configuration`

---

### Task 5: `ImageStore`; serving and deleting images

**Files:**
- Create: `services/src/infrastructure/image_store.rs`, `services/src/test_support/image_store.rs`, `services/tests/fs_image_store.rs`
- Modify: `services/src/services/images.rs` (`get_local`, `delete_unused_image`, `save_tmdb_image`'s first write, `save_image_size`), callers of `delete_unused_image` in `media.rs`, `api/src/endpoints/images.rs` (`get_local`), `AppState`, `launch()`
- Test: `images.rs`, `services/tests/fs_image_store.rs`

**Interfaces:**
- Produces:
```rust
#[async_trait]
pub trait ImageStore: Send + Sync {
    /// Creates `directory` if needed. Fails if the file already exists. Removes a partial file on write failure.
    async fn write_new(&self, directory: &str, file_name: &str, bytes: Vec<u8>) -> Result<(), SrvErr>;
    /// `SrvErr::NotFound` when the file doesn't exist.
    async fn open(&self, directory: &str, file_name: &str) -> Result<BoxStream<'static, std::io::Result<Bytes>>, SrvErr>;
    /// Removes the directory and everything in it.
    async fn delete_directory(&self, directory: &str) -> Result<(), SrvErr>;
}
pub struct FsImageStore { root: PathBuf } impl FsImageStore { pub fn new(root: impl Into<PathBuf>) -> Self; }
```
  - `file_name` is `"{width}x{height}.{extension}"`, the same layout as today: `<root>/<directory>/<w>x<h>.<ext>`.
  - `test_support::InMemoryImageStore::new()`, `.insert(dir, name, bytes)`, `.files() -> Vec<(String, String)>` (sorted), `.get(dir, name) -> Option<Vec<u8>>`. Uses a `std::sync::Mutex`.
  - New signatures:
    - `images::get_local(slug, name, store) -> Result<(&'static str, BoxStream<'static, std::io::Result<Bytes>>), SrvErr>`
    - `images::delete_unused_image(id, db, store)`
    - `images::save_tmdb_image(tmdb_path, type, db, tmdb, store)`, plus `jobs`/`random` in Tasks 6–7
  - Callers (`media::update_backdrop`, `update_poster`, `delete`, …) gain `store` and pass it through.
  - `AppState.image_store: Arc<dyn ImageStore>`, built as `FsImageStore::new(config().images.store_path.clone())`.

- [ ] **Step 1: Write failing tests**
  - `get_local_returns_bad_request_when_name_not_32_hex`, `get_local_returns_bad_request_when_slug_not_dimensions`, `get_local_returns_bad_request_when_extension_unknown`, `get_local_returns_not_found_when_file_missing`, `get_local_streams_file_with_mime_type` (`.webp` → `"image/webp"`)
  - `delete_unused_image_keeps_image_when_media_uses_it` (store untouched, `assert_statement_count`)
  - `delete_unused_image_keeps_image_when_season_or_episode_uses_it`
  - `delete_unused_image_removes_rows_and_directory_when_unused`
  - `fetch_backdrop_and_poster_assigns_by_image_type`
  - `services/tests/fs_image_store.rs`, all `#[ignore = "touches filesystem"]`, using `tempfile::tempdir()`:
    - `write_new_creates_directory_and_file`
    - `write_new_fails_when_file_exists`
    - `open_returns_not_found_when_missing`
    - `open_streams_written_bytes`
    - `delete_directory_removes_all_files`

- [ ] **Step 2: Run** `cargo test -p services` and confirm the compile failure.

- [ ] **Step 3: Implement the trait, `FsImageStore` (ported from today's `tokio::fs` code), `InMemoryImageStore`, and the signature and `api` changes. Add `tempfile = "3"` to `services` dev-deps.**

- [ ] **Step 4: Run** `cargo test -p services` (expected: pass, fs tests listed as ignored) and `cargo test -p services --test fs_image_store -- --ignored` (expected: 5 pass).

- [ ] **Step 5: Commit** — `Put image files behind ImageStore; cover image serving and deletion`

---

### Task 6: Typed jobs; flatten image size saving

**Files:**
- Create: `services/src/jobs.rs`, `services/src/test_support/jobs.rs`
- Modify: `services/src/services/images.rs` (remove `save_image_sizes_task` and the `async_scoped` usage; `save_tmdb_image` submits jobs; add the `SaveImageSize` handler), `services/Cargo.toml` (drop `async-scoped` if unused), `services/src/lib.rs` (`pub mod jobs;`), `AppState`, `launch()`
- Test: `images.rs`, `jobs.rs`

**Interfaces:**
- Produces:
```rust
#[derive(Debug, Clone)]
pub enum Job {
    SaveImageSize { image_id: i32, width: u32, height: u32, directory: String,
                    extension: &'static str, format: ImageFormat, image: Arc<DynamicImage> },
}
#[async_trait] pub trait Jobs: Send + Sync { async fn submit(&self, job: Job); }
pub struct JobDeps { pub db: DbConn, pub store: Arc<dyn ImageStore> }
pub async fn run(job: Job, deps: &JobDeps) -> Result<(), SrvErr>;     // one match arm per variant
pub struct TokioJobs { deps: Arc<JobDeps> }
impl TokioJobs { pub fn new(deps: JobDeps) -> Self; }                 // submit = tokio::spawn(run), logs Err with error!
```
  - `images::save_image_size(image_id, width, height, directory: &str, extension, format, image: &DynamicImage, db, store) -> Result<(), SrvErr>` (`pub(crate)`). `run` calls it for `SaveImageSize`.
  - `save_tmdb_image(tmdb_path, type, db, tmdb, store, jobs)`. `random` is added in Task 7. It saves the original size synchronously, as today. It then computes the remaining sizes with today's algorithm (`gcd`, thresholds `100 + 200*i` while `threshold <= short - 200`, `calculate_image_size`), and submits one `Job::SaveImageSize` per size, smallest first, sharing one `Arc<DynamicImage>`.
  - `test_support::RecordingJobs::new()`, `.take() -> Vec<Job>`.
  - `test_support::InlineJobs::new(deps: JobDeps)`: `submit` awaits `run` and panics with the error on `Err`.
  - `test_support::jobs::sizes(jobs: &[Job]) -> Vec<(i32, u32, u32)>` returns `(image_id, width, height)` per job.
  - `AppState.jobs: Arc<dyn Jobs>`, built as `TokioJobs::new(JobDeps { db: conn.clone(), store: image_store.clone() })`.

- [ ] **Step 1: Write failing tests**
  - `save_tmdb_image_returns_existing_id_when_source_path_known` (no TMDB call: `MockTmdb` with no expectations; no jobs; store empty)
  - `save_tmdb_image_saves_original_and_submits_remaining_sizes`: `MockTmdb::image` returns `png_bytes(1000, 1500)`, and the mock DB returns image id `7`. Assert:
    - the store holds exactly `(<dir>, "1000x1500.png")`;
    - an `images` insert with `source_path` = the TMDB path;
    - one `image_sizes` insert `(7, 1000, 1500)`;
    - `sizes(&jobs.take()) == vec![(7,100,150),(7,300,450),(7,500,750),(7,700,1050)]`.
  - `save_tmdb_image_submits_no_jobs_when_short_side_at_most_300` (image `300×450` → `[]`)
  - `save_tmdb_image_does_not_panic_on_tiny_image` (`150×150`; if it panics today, apply the known-bug rule)
  - `save_tmdb_image_returns_integration_error_when_bytes_not_an_image`
  - `run_save_image_size_writes_resized_file_and_row`: take the jobs recorded by the 1000×1500 test setup, then run each with `jobs::run(job, &JobDeps { db: mock_db, store })` **sequentially**. Assert the store gained `100x150.png`, `300x450.png`, `500x750.png`, `700x1050.png`, that 4 `image_sizes` inserts were made with those dimensions, and that the decoded files have those dimensions.
  - `run_save_image_size_returns_err_when_file_exists` (store pre-seeded)

- [ ] **Step 2: Run** `cargo test -p services` and confirm the compile failure.

- [ ] **Step 3: Implement `jobs.rs`, the test jobs, the flattened `save_tmdb_image` and the handler. Thread `jobs` through `save_tmdb_image`'s callers to `api`.**

- [ ] **Step 4: Run** `cargo test -p services && cargo build --workspace`. Expected: pass and build.

- [ ] **Step 5: Commit** — `Replace spawned image resizing with typed jobs`

---

### Task 7: Remove the remaining direct time and randomness from TMDB-backed paths

**Files:**
- Modify: `services/src/infrastructure/integrations_trait_implementations.rs:50,121` (`date_added: NotSet`), `services/src/services/{images.rs,movies.rs,series.rs,media.rs,refresh.rs,logset.rs}`, `api` callers

**Interfaces:**
- Produces the final signatures used by Tasks 8–11:
  - `images::save_tmdb_image(tmdb_path, type, db, tmdb, store, jobs, random)`. The directory name is `random.uuid().simple().to_string()`, looping on conflict as today.
  - `media::fetch_id_or_pull_image(source, path, image_type, db, tmdb, store, jobs, random)`
  - `movies::create(tmdb_id, user, db, tmdb, store, jobs, clock, random)`. `date_added` comes from `clock.now().date_naive()`.
  - `series::create(tmdb_id, user, db, tmdb, store, jobs, clock, random)`
  - `media::create_w_source(view, media_type, user, db, tmdb, store, jobs, clock, random)`
  - `logset::create(logset, user, db, tmdb, store, jobs, clock, random)`
  - `media::apply_changes(media, changes, db, tmdb, store, jobs, random)`
  - `movies::apply_changes(media, changes, db, tmdb, store, jobs, random)`
  - `series::apply_changes(media, changes, start_date, end_date, db, tmdb, store, jobs, random)`
  - `media::update_backdrop` / `update_poster` / `update_images(…, db, tmdb, store, jobs, random)`
  - `media::delete(media_id, media_type, user, db, store)`
  - `media::delete_w_source(tmdb_id, source_name, media_type, user, db, store)`
  - `refresh::refresh(db, tmdb, store, jobs, clock, random)`. `run_refresh()` builds `SystemClock`, `OsRandom`, `TmdbClient`, `FsImageStore` and `TokioJobs` itself.
  - `media::backdrops` / `posters` / `images(…, db, tmdb)`
  - `media::search(query, committed, page_req, media_type, user, db, tmdb)`
  - `watchlist::search` is unchanged unless it reaches TMDB.
  - Exact parameter sets follow the Global Constraints order. Where the call graph needs fewer or more than listed, follow the call graph.

- [ ] **Step 1: Make the changes; confirm with grep**

Run: `grep -rn -e "Utc::now" -e "new_v4" -e "OsRng" -e "task::spawn" -e "tokio::spawn" -e "tokio::fs" -e "std::fs" services/src | grep -v -e "test_support" -e "infrastructure/clock.rs" -e "infrastructure/random.rs" -e "infrastructure/image_store.rs" -e "src/jobs.rs" -e "fn run_refresh" -e "fn run_linearize_ratings"`
Expected: no output, apart from lines inside the bodies of `run_refresh`/`run_linearize_ratings`. Check those by eye.

- [ ] **Step 2: Run** `cargo test --workspace && cargo clippy --workspace --all-targets`. Expected: pass, no new warnings.

- [ ] **Step 3: Commit** — `Inject clock and randomness into TMDB-backed services`

---

### Task 8: Cover movies

**Files:** Test: `services/src/services/movies.rs`

- [ ] **Step 1: Write tests for every public function**
  - `create_returns_existing_id_when_movie_already_added`: no TMDB calls, `(false, id)`
  - `create_inserts_media_movie_titles_and_genres`: `MockTmdb` `movie_details` + `movie_alternative_titles`, backdrop and poster paths; assert `(true, id)`, the `media` insert has `date_added == FixedClock date`, `user_id == 1`, `bot_controllable == false`; the `movies` insert; `media_genres` inserts; 2 images saved; jobs recorded
  - `create_returns_masterdata_outdated_when_genre_unknown`
  - `create_returns_err_and_writes_nothing_when_tmdb_details_fail` (Review Focus 5)
  - `index_returns_page_of_movie_indexes`
  - `details_returns_none_when_missing`
  - `details_assembles_titles_genres_tags_sources_logs_and_images`
  - `details_uses_unset_title_when_no_primary_title` (`"Unknown"`)
  - `metadata_returns_current_values_and_locks`
  - `update_writes_changed_fields_and_locks_them`
  - `update_returns_not_found_when_media_belongs_to_another_user`
  - `lock_sets_movie_lock`, `unlock_clears_movie_lock`
  - `lock_returns_bad_request_when_property_unknown` (or whatever `set_lock` returns today)
  - `apply_changes_updates_unlocked_properties`
  - `apply_changes_skips_locked_properties`

- [ ] **Step 2: Run** `cargo test -p services movies::`. Expected: pass, or failures handled by the known-bug rule.

- [ ] **Step 3: Commit** — `Cover movies service`

---

### Task 9: Cover series

**Files:** Test: `services/src/services/series.rs`

- [ ] **Step 1: Write tests for every public function, plus the private season/episode helpers through their public entry points**
  - `create`: the same four cases as movies, plus `create_inserts_seasons_and_episodes_from_season_details` (2 seasons × 2 episodes; season poster and episode still images saved; jobs recorded)
  - `index`, `details` (with seasons), `metadata`, `update`, `lock`, `unlock`: as for movies
  - `apply_changes_upserts_new_season_when_season_added`
  - `apply_changes_applies_season_property_changes`
  - `apply_changes_applies_episode_changes`
  - `apply_changes_skips_locked_properties`

- [ ] **Step 2: Run** `cargo test -p services series::`. Expected: pass, or the known-bug rule.

- [ ] **Step 3: Commit** — `Cover series service`

---

### Task 10: Cover media, logset, watchlist reads

**Files:** Test: `services/src/services/{media.rs,logset.rs,watchlist.rs}`

- [ ] **Step 1: Write tests**
  - `media`:
    - `create_w_source_creates_movie_and_source`, `create_w_source_creates_series_and_source`
    - `index_returns_page`
    - `search_by_ids_returns_matching_media`
    - `search_returns_internal_results_for_committed_query`
    - `search_includes_tmdb_results_when_not_committed`
    - `search_returns_bad_request_when_query_unparseable`
    - `on_watchlist_true_and_false` (two tests)
    - `backdrops_returns_tmdb_candidates_with_backdrop_sizes`
    - `posters_returns_tmdb_candidates_with_poster_sizes`
    - `images_returns_both`
    - `update_backdrop_pulls_image_and_deletes_unused_previous`
    - `update_poster_…` (same)
    - `update_images_sets_both`
    - `delete_w_source_removes_source_and_media_when_last_source`
    - `delete_w_source_keeps_media_when_other_sources_remain`
    - `delete_removes_media_and_unused_images`
    - `apply_changes_updates_unlocked_media_properties`
    - `apply_changes_replaces_backdrop_when_backdrop_path_changed`
    - `is_locked_…` (pure)
    - `build_media_index_uses_unset_title_when_none`
    - `try_set_media_lock_…`
    - `fetch_media_returns_not_found_when_media_belongs_to_another_user`
  - `logset`: `create_creates_media_when_missing_then_adds_log`, `create_uses_existing_media`
  - `watchlist`: `index_returns_page`, `search_filters_to_watchlist`

- [ ] **Step 2: Run** `cargo test -p services`. Expected: pass, or the known-bug rule.

- [ ] **Step 3: Commit** — `Cover media, logset and watchlist services`

---

### Task 11: Cover refresh, statistics, ratings linearization (DB part)

**Files:** Test: `services/src/services/{refresh.rs,statistics.rs,ratings_linearization.rs}`

- [ ] **Step 1: Write tests**
  - `refresh`:
    - `refresh_uses_earliest_date_added_when_never_synced`
    - `refresh_uses_last_sync_date_when_synced`: `FixedClock`; assert `movie_changed`/`series_changed` are called with the exact `(start, end)` dates
    - `refresh_applies_changes_only_for_tracked_tmdb_ids`
    - `refresh_counts_errors_without_aborting` (one `movie_property_changes` returns `Err`; `RefreshResult { updates, errors: 1 }`)
    - `refresh_records_sync_state_at_now`
    - `build_intervals_…` (pure)
  - `statistics`: `stats_aggregates_all_counters` (queue the 15 query results in the order the `try_join!` polls them, which is the source order)
  - `ratings_linearization`:
    - `linearize_ratings_returns_not_found_when_user_uuid_unknown`
    - `linearize_ratings_dry_run_writes_nothing`
    - `linearize_ratings_updates_log_ratings_and_media_stars`
    - The 3 existing pure tests stay.

- [ ] **Step 2: Run** `cargo test -p services`. Expected: pass, or the known-bug rule.

- [ ] **Step 3: Commit** — `Cover refresh, statistics and ratings linearization`

---

### Task 12: Postgres integration tests (testcontainers)

**Files:**
- Create: `services/tests/common/mod.rs`, `services/tests/common/postgres.rs`, `services/tests/postgres_migrations.rs`, `postgres_media.rs`, `postgres_search.rs`, `postgres_stats.rs`, `postgres_refresh.rs`, `postgres_crud.rs`
- Modify: `services/Cargo.toml` (dev-deps: `testcontainers` with the `blocking` feature, `testcontainers-modules` with `postgres`, at the latest mutually compatible versions; `migrations = { path = "../migrations" }`)

**Interfaces:**
- Produces: `common::postgres::database() -> DatabaseConnection` (async). On first call per binary it:
  1. starts one `testcontainers_modules::postgres::Postgres` container with `SyncRunner`, held in a `static OnceLock` (never dropped; Ryuk reaps it);
  2. calls `infrastructure::init_config_once` with `test_config()` but `db.connection_url` = the container's `mia_template` database URL (needed because `m20260417_000002_*` reads it);
  3. calls `init_tmdb_config_once(&test_tmdb_configuration())`;
  4. creates `mia_template` and runs `migration::Migrator::up(&conn, None)` on it, on a dedicated thread with its own Tokio runtime so it doesn't nest inside the test's runtime, then closes every connection to it.

  Every call (including the first) then runs `CREATE DATABASE test_<atomic counter> TEMPLATE mia_template` and returns a connection to it.
- Every test is `#[ignore = "requires docker"]` and uses `MockTmdb` / `InMemoryImageStore` / `InlineJobs` / `FixedClock` / `SeqRandom`. A test-wide `MockTmdb` panic means a test missed an expectation.

- [ ] **Step 1: Write tests**
  - `postgres_migrations.rs`: `migrations_apply_to_empty_database`, `entities_match_schema`. The second does a `find().one()` on every entity in `entities::prelude` against the empty DB; an `Err` means drift.
  - `postgres_media.rs`:
    - `movie_create_persists_movie_with_images_and_sizes`: `InlineJobs`, `png_bytes(1000,1500)`; assert 5 `image_sizes` rows
    - `series_create_persists_seasons_and_episodes`
    - `media_delete_removes_rows_and_unused_images`
  - `postgres_search.rs`: one test per search operator group the `transpiler` supports (read `transpiler/src/parser.rs` for the list). Assert matching media are returned. This is the main point of Postgres tests: the transpiler's SQL must run.
  - `postgres_stats.rs`: `stats_runs_against_real_schema` (seed a movie, a series and logs; assert the counters)
  - `postgres_refresh.rs`: `refresh_applies_movie_property_changes`, `linearize_ratings_updates_real_rows`
  - `postgres_crud.rs`: one round trip each for tags, titles, sources, logs, watchlist, app tokens, users (register → login), `masterdata::refresh`

- [ ] **Step 2: Run** `cargo test -p services --tests -- --ignored` (Docker running). Expected: all pass. A failure that's a real bug goes through the known-bug rule (it stays ignored with its known-bug reason instead of `"requires docker"`).

- [ ] **Step 3: Run** `cargo test -p services` without Docker. Expected: pass, Postgres tests shown as ignored.

- [ ] **Step 4: Commit** — `Add Postgres integration tests with testcontainers`

---

### Task 13: TMDB contract tests

**Files:**
- Create: `integrations/tests/tmdb_contract.rs`
- Modify: `integrations/Cargo.toml` (dev-deps: `tokio` with `macros`/`rt`, `serde_yaml`, `chrono`)

**Interfaces:**
- Consumes: `TmdbClient::new` (Task 4).
- Produces: the test-local fn `client() -> TmdbClient`, which reads `authorization_token` from `concat!(env!("CARGO_MANIFEST_DIR"), "/../config.yaml")` via a minimal `serde_yaml` struct, and panics with `"config.yaml with tmdb.authorization_token required for TMDB contract tests"` if it's missing.

- [ ] **Step 1: Write tests**, all `#[ignore = "requires TMDB token"]`, one per `Tmdb` method:
  - IDs: movie `550` (Fight Club), series `1396` (Breaking Bad), season `1396`/`1`, season id `3572`, episode id `62085`.
  - Assert `Ok` and key fields: `movie_details(550).title == "Fight Club"`, `series_details(1396).name == "Breaking Bad"`, season 1 has 7 episodes, `movie_genres` is non-empty, `configuration().images.secure_base_url` starts with `"https://"`, and so on.
  - Changes endpoints use the window `[today-3, today]`. `movie_changed` must return more than 100 ids, which proves pagination.
  - `image(movie_details(550).poster_path)` decodes with `image::load_from_memory`. Add `image` as a dev-dep.
  - `image_stream("w92", path)` returns status 200 and an `image/…` content type.

- [ ] **Step 2: Run** `cargo test -p integrations -- --ignored` (with `config.yaml`). Expected: all pass.

- [ ] **Step 3: Commit** — `Add TMDB contract tests`

---

### Task 14: Document testing in CLAUDE.md; final verification

**Files:** Modify: `CLAUDE.md`

- [ ] **Step 1: Add a `## Testing` section**
  - Commands: `cargo test` (unit, no environment), `cargo test -- --ignored` (all real integration tests), `cargo test -p services --tests -- --ignored` (Postgres + filesystem; needs Docker), `cargo test -p integrations -- --ignored` (TMDB; needs `config.yaml`).
  - The dependency traits and their test fakes (`MockTmdb`, `InMemoryImageStore`, `RecordingJobs`/`InlineJobs`, `FixedClock`, `SeqRandom`) and where they live (`services::test_support`).
  - Rules: parameter order; no direct `Utc::now`/`Uuid::new_v4`/`OsRng`/spawn/fs in `services`; the record-then-run-sequentially job pattern; ignore reasons; the known-bug rule and where the list lives; test naming.

- [ ] **Step 2: Verify**
  - Run: `cargo test --workspace`. Expected: pass; the ignored count equals the number of integration tests plus known bugs.
  - Run: `cargo test --workspace -- --ignored` (Docker + `config.yaml`). Expected: only the `known bug:` tests fail.
  - Run: `cargo clippy --workspace --all-targets`. Expected: no new warnings.

- [ ] **Step 3: Commit** — `Document testing workflow`
