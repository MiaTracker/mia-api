//! Shared support code for unit tests: fixed configuration, mock database helpers, fixtures, an
//! in-memory image store, recording and inline jobs, a mock TMDB and a mock LLM.
//!
//! Compiled for this crate's own tests and for other crates enabling the `test-support` feature.

pub mod config;
pub mod db;
pub mod errors;
pub mod fixtures;
pub mod image_store;
pub mod jobs;
pub mod llm;
pub mod time;
pub mod tmdb;

pub use errors::{assert_single_violation, violations};
pub use image_store::InMemoryImageStore;
pub use jobs::{InlineJobs, RecordingJobs};
pub use llm::MockLlm;
pub use time::{date, fixed_clock, FixedClock, SeqRandom};
pub use tmdb::MockTmdb;

/// Initializes the process-wide configuration globals with fixed test values.
///
/// Idempotent and independent of `config.yaml`. Call it first in every test that touches
/// code reading `infrastructure::config()` or `tmdb_config()`.
pub fn init() {
    infrastructure::init_config_once(config::test_config);
    crate::infrastructure::constants::init_tmdb_config_once(&config::test_tmdb_configuration());
}
