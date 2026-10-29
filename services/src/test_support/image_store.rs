use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use futures::stream::BoxStream;
use crate::infrastructure::{ImageStore, SrvErr};

type Files = BTreeMap<(String, String), Vec<u8>>;

/// An `ImageStore` that keeps files in memory. Clones share the same files.
#[derive(Clone, Default)]
pub struct InMemoryImageStore {
    files: Arc<Mutex<Files>>,
}

impl InMemoryImageStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts a file in the store, replacing any file with the same name.
    pub fn insert(&self, directory: &str, file_name: &str, bytes: Vec<u8>) {
        self.files_locked().insert((directory.to_string(), file_name.to_string()), bytes);
    }

    /// Every stored file as `(directory, file_name)`, sorted.
    pub fn files(&self) -> Vec<(String, String)> {
        self.files_locked().keys().cloned().collect()
    }

    pub fn get(&self, directory: &str, file_name: &str) -> Option<Vec<u8>> {
        self.files_locked().get(&(directory.to_string(), file_name.to_string())).cloned()
    }

    fn files_locked(&self) -> std::sync::MutexGuard<'_, Files> {
        self.files.lock().expect("image store lock")
    }
}

#[async_trait]
impl ImageStore for InMemoryImageStore {
    async fn write_new(&self, directory: &str, file_name: &str, bytes: Vec<u8>) -> Result<(), SrvErr> {
        let mut files = self.files_locked();
        let key = (directory.to_string(), file_name.to_string());
        if files.contains_key(&key) {
            return Err(SrvErr::from(std::io::Error::from(ErrorKind::AlreadyExists)));
        }
        files.insert(key, bytes);
        Ok(())
    }

    async fn open(&self, directory: &str, file_name: &str) -> Result<BoxStream<'static, std::io::Result<Bytes>>, SrvErr> {
        let bytes = self.get(directory, file_name).ok_or(SrvErr::NotFound)?;
        Ok(futures::stream::iter([Ok(Bytes::from(bytes))]).boxed())
    }

    /// Like the filesystem store, fails when the directory doesn't exist (holds no files).
    async fn delete_directory(&self, directory: &str) -> Result<(), SrvErr> {
        let mut files = self.files_locked();
        let before = files.len();
        files.retain(|(dir, _), _| dir != directory);
        if files.len() == before {
            return Err(SrvErr::Internal("Failed to delete image directory".to_string()));
        }
        Ok(())
    }
}
