//! Support shared by the Postgres integration tests. Each test binary uses a different part of it.
#![allow(dead_code)]

pub mod postgres;
pub mod seed;

use std::sync::Arc;
use sea_orm::DbConn;
use services::jobs::JobDeps;
use services::test_support::{fixed_clock, FixedClock, InMemoryImageStore, InlineJobs, SeqRandom};

/// The fakes a service call needs besides the database and TMDB. Jobs run inline against `db`.
pub struct Fakes {
    pub store: InMemoryImageStore,
    pub jobs: InlineJobs,
    pub clock: FixedClock,
    pub random: SeqRandom,
}

/// Fakes for `db`, with the clock at 2024-01-01 12:00 UTC.
pub fn fakes(db: &Arc<DbConn>) -> Fakes {
    let store = InMemoryImageStore::new();
    let jobs = InlineJobs::new(JobDeps { db: db.clone(), store: Arc::new(store.clone()) });
    Fakes { store, jobs, clock: fixed_clock(2024, 1, 1, 12, 0, 0), random: SeqRandom::new() }
}
