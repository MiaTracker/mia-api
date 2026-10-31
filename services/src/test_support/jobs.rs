//! `Jobs` implementations for tests.
//!
//! Unit tests use `RecordingJobs`: call the code under test, assert the jobs it submitted, then run
//! each one with `jobs::run` **one at a time** against the mock DB and fakes. Integration tests
//! against a real database use `InlineJobs`, which runs each job as it is submitted.

use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use sea_orm::DbConn;
use crate::chat::registry::ExchangeRegistry;
use crate::infrastructure::ImageStore;
use crate::jobs::{run, Job, JobDeps, Jobs};
use super::{fixed_clock, MockLlm, MockTmdb};

/// Keeps submitted jobs without running them.
#[derive(Default)]
pub struct RecordingJobs {
    jobs: Mutex<Vec<Job>>,
}

impl RecordingJobs {
    pub fn new() -> Self {
        Self::default()
    }

    /// The jobs submitted so far, in submission order. Leaves none behind.
    pub fn take(&self) -> Vec<Job> {
        std::mem::take(&mut *self.jobs.lock().expect("recorded jobs lock"))
    }
}

#[async_trait]
impl Jobs for RecordingJobs {
    async fn submit(&self, job: Job) {
        self.jobs.lock().expect("recorded jobs lock").push(job);
    }
}

/// Runs each job to completion when it is submitted. Panics if a job fails.
pub struct InlineJobs {
    deps: JobDeps,
}

impl InlineJobs {
    pub fn new(deps: JobDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl Jobs for InlineJobs {
    async fn submit(&self, job: Job) {
        if let Err(err) = run(job, &self.deps).await {
            panic!("inline job failed: {err}");
        }
    }
}

/// `JobDeps` for `db` and `store`, with a `MockLlm` and a `MockTmdb` expecting no calls, a new
/// registry and the clock at 2024-01-01 12:00 UTC.
pub fn job_deps(db: Arc<DbConn>, store: Arc<dyn ImageStore>) -> JobDeps {
    JobDeps {
        db,
        store,
        llm: Arc::new(MockLlm::new()),
        registry: Arc::new(ExchangeRegistry::new()),
        tmdb: Arc::new(MockTmdb::new()),
        clock: Arc::new(fixed_clock(2024, 1, 1, 12, 0, 0)),
    }
}

/// `(image_id, width, height)` of each `SaveImageSize` job, in order; other jobs are left out.
pub fn sizes(jobs: &[Job]) -> Vec<(i32, u32, u32)> {
    jobs.iter()
        .filter_map(|job| match job {
            Job::SaveImageSize { image_id, width, height, .. } => Some((*image_id, *width, *height)),
            Job::RunExchange { .. } => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use image::{DynamicImage, ImageFormat};
    use super::*;
    use crate::test_support;
    use crate::test_support::{fixtures, InMemoryImageStore};

    fn job(width: u32, height: u32) -> Job {
        Job::SaveImageSize {
            image_id: 7, width, height, directory: "image-7".to_string(), extension: "png",
            format: ImageFormat::Png, image: Arc::new(DynamicImage::new_rgb8(200, 300)),
        }
    }

    #[tokio::test]
    async fn recording_jobs_take_returns_submitted_jobs_in_order_and_empties() {
        let jobs = RecordingJobs::new();

        jobs.submit(job(100, 150)).await;
        jobs.submit(job(20, 30)).await;

        assert_eq!(sizes(&jobs.take()), vec![(7, 100, 150), (7, 20, 30)]);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn inline_jobs_submit_runs_job_when_submitted() {
        let store = InMemoryImageStore::new();
        let db = test_support::db::mock()
            .append_query_results([[fixtures::image_size_model(1, 7, 100, 150)]])
            .into_connection();
        let jobs = InlineJobs::new(job_deps(Arc::new(db), Arc::new(store.clone())));

        jobs.submit(job(100, 150)).await;

        assert_eq!(store.files(), vec![("image-7".to_string(), "100x150.png".to_string())]);
    }

    #[tokio::test]
    #[should_panic(expected = "inline job failed")]
    async fn inline_jobs_submit_panics_when_job_fails() {
        let store = InMemoryImageStore::new();
        store.insert("image-7", "100x150.png", b"existing".to_vec());
        let db = test_support::db::mock().into_connection();
        let jobs = InlineJobs::new(job_deps(Arc::new(db), Arc::new(store)));

        jobs.submit(job(100, 150)).await;
    }
}
