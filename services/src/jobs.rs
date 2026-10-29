//! Background work as typed values.
//!
//! Service code never spawns. It submits a `Job` through `&dyn Jobs`, and the implementation
//! decides when it runs: `TokioJobs` spawns it (production), `test_support::RecordingJobs` keeps it
//! for the test to inspect and run, and `test_support::InlineJobs` runs it on the spot.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use async_trait::async_trait;
use futures::FutureExt;
use image::{DynamicImage, ImageFormat};
use log::error;
use sea_orm::DbConn;
use crate::infrastructure::{ImageStore, SrvErr};
use crate::images;

/// A unit of background work. Each variant owns the data its handler needs.
///
/// `Debug` prints `describe()`, leaving out image data.
#[derive(Clone)]
pub enum Job {
    /// Resize `image` to `width` x `height`, write it to `directory` and insert its `image_sizes` row.
    SaveImageSize {
        image_id: i32,
        width: u32,
        height: u32,
        directory: String,
        extension: &'static str,
        format: ImageFormat,
        image: Arc<DynamicImage>,
    },
}

impl Job {
    /// A short description for logs; leaves out the image data.
    fn describe(&self) -> String {
        match self {
            Job::SaveImageSize { image_id, width, height, .. } => {
                format!("SaveImageSize (image {image_id}, {width}x{height})")
            }
        }
    }
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}

/// Where service code submits background work.
#[async_trait]
pub trait Jobs: Send + Sync {
    async fn submit(&self, job: Job);
}

/// What job handlers need.
pub struct JobDeps {
    /// Shared because `DatabaseConnection` is not `Clone` when sea-orm's `mock` feature is on (test builds).
    pub db: Arc<DbConn>,
    pub store: Arc<dyn ImageStore>,
}

/// Runs `job` to completion.
pub async fn run(job: Job, deps: &JobDeps) -> Result<(), SrvErr> {
    match job {
        Job::SaveImageSize { image_id, width, height, directory, extension, format, image } => {
            images::save_image_size(image_id, width, height, &directory, extension, format, &image,
                                    &deps.db, deps.store.as_ref()).await
        }
    }
}

/// Runs each submitted job in its own Tokio task and logs failures.
pub struct TokioJobs {
    deps: Arc<JobDeps>,
}

impl TokioJobs {
    pub fn new(deps: JobDeps) -> Self {
        Self { deps: Arc::new(deps) }
    }
}

#[async_trait]
impl Jobs for TokioJobs {
    async fn submit(&self, job: Job) {
        let deps = self.deps.clone();
        tokio::spawn(async move {
            let description = job.describe();
            match AssertUnwindSafe(run(job, &deps)).catch_unwind().await {
                Ok(Ok(())) => {}
                Ok(Err(err)) => error!("Background job {} failed: {}", description, err),
                Err(panic) => error!("Background job {} panicked: {}", description, panic_message(panic.as_ref())),
            }
        });
    }
}

/// The message a panic carried, if it was a string.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> &str {
    panic.downcast_ref::<&str>().copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("non-string panic payload")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_debug_leaves_out_image_data_when_formatted() {
        let job = Job::SaveImageSize {
            image_id: 7, width: 100, height: 150, directory: "image-7".to_string(), extension: "png",
            format: ImageFormat::Png, image: Arc::new(DynamicImage::new_rgb8(200, 300)),
        };

        assert_eq!(format!("{job:?}"), "SaveImageSize (image 7, 100x150)");
    }
}
