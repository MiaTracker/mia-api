use std::io::ErrorKind;
use std::path::PathBuf;
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use log::error;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use crate::infrastructure::SrvErr;

/// Where image files live. Services take `&dyn ImageStore` instead of touching the filesystem,
/// so tests can use an in-memory store.
///
/// Files are addressed by `directory` (the image's directory name) and `file_name`
/// (`"{width}x{height}.{extension}"`).
#[async_trait]
pub trait ImageStore: Send + Sync {
    /// Creates `directory` if needed. Fails if the file already exists. Removes a partial file on write failure.
    async fn write_new(&self, directory: &str, file_name: &str, bytes: Vec<u8>) -> Result<(), SrvErr>;

    /// `SrvErr::NotFound` when the file doesn't exist.
    async fn open(&self, directory: &str, file_name: &str) -> Result<BoxStream<'static, std::io::Result<Bytes>>, SrvErr>;

    /// Removes the directory and everything in it.
    async fn delete_directory(&self, directory: &str) -> Result<(), SrvErr>;
}

/// Stores images as files under `<root>/<directory>/<file_name>`.
pub struct FsImageStore {
    root: PathBuf,
}

impl FsImageStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
}

#[async_trait]
impl ImageStore for FsImageStore {
    async fn write_new(&self, directory: &str, file_name: &str, bytes: Vec<u8>) -> Result<(), SrvErr> {
        let directory_path = self.root.join(directory);
        fs::create_dir_all(&directory_path).await?;

        let path = directory_path.join(file_name);
        let mut file = File::create_new(&path).await.map_err(|e| {
            error!("Could not create new file {}: {}", path.display(), e);
            e
        })?;
        if let Err(err) = file.write_all(bytes.as_slice()).await {
            drop(file);
            let _ = fs::remove_file(path).await;
            return Err(SrvErr::from(err));
        }
        file.flush().await?;
        Ok(())
    }

    async fn open(&self, directory: &str, file_name: &str) -> Result<BoxStream<'static, std::io::Result<Bytes>>, SrvErr> {
        let path = self.root.join(directory).join(file_name);
        match File::open(&path).await {
            Ok(file) => Ok(ReaderStream::new(file).boxed()),
            Err(err) => match err.kind() {
                ErrorKind::NotFound => Err(SrvErr::NotFound),
                _ => {
                    error!("Error reading image file {}: {}", path.display(), err);
                    Err(SrvErr::Internal("Error reading image file".to_string()))
                }
            }
        }
    }

    async fn delete_directory(&self, directory: &str) -> Result<(), SrvErr> {
        fs::remove_dir_all(self.root.join(directory)).await.map_err(|e| {
            error!("Failed to delete directory: {:?}", e);
            SrvErr::Internal("Failed to delete image directory".to_string())
        })
    }
}
