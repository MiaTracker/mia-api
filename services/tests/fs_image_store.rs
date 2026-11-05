//! Tests of the real filesystem-backed image store. They write to temporary directories, so
//! they are ignored by default; run them with `cargo test -p services --test fs_image_store -- --ignored`.

use bytes::Bytes;
use futures::TryStreamExt;
use services::infrastructure::{FsImageStore, ImageStore, SrvErr};

const DIRECTORY: &str = "0123456789abcdef0123456789abcdef";

#[tokio::test]
#[ignore = "touches filesystem"]
async fn write_new_creates_directory_and_file() {
    let root = tempfile::tempdir().expect("temp dir");
    let store = FsImageStore::new(root.path());

    store.write_new(DIRECTORY, "300x450.jpg", b"jpeg-bytes".to_vec()).await.expect("write_new should succeed");

    let written = std::fs::read(root.path().join(DIRECTORY).join("300x450.jpg")).expect("file should exist");
    assert_eq!(written, b"jpeg-bytes");
}

#[tokio::test]
#[ignore = "touches filesystem"]
async fn write_new_fails_when_file_exists() {
    let root = tempfile::tempdir().expect("temp dir");
    let store = FsImageStore::new(root.path());
    store.write_new(DIRECTORY, "300x450.jpg", b"first".to_vec()).await.expect("first write should succeed");

    let result = store.write_new(DIRECTORY, "300x450.jpg", b"second".to_vec()).await;

    assert!(matches!(result, Err(SrvErr::IO(_))), "expected an IO error");
    let kept = std::fs::read(root.path().join(DIRECTORY).join("300x450.jpg")).expect("file should still exist");
    assert_eq!(kept, b"first");
}

#[tokio::test]
#[ignore = "touches filesystem"]
async fn open_returns_not_found_when_missing() {
    let root = tempfile::tempdir().expect("temp dir");
    let store = FsImageStore::new(root.path());

    let result = store.open(DIRECTORY, "300x450.jpg").await;

    assert!(matches!(result, Err(SrvErr::NotFound)), "expected NotFound");
}

#[tokio::test]
#[ignore = "touches filesystem"]
async fn open_streams_written_bytes() {
    let root = tempfile::tempdir().expect("temp dir");
    let store = FsImageStore::new(root.path());
    store.write_new(DIRECTORY, "300x450.jpg", b"jpeg-bytes".to_vec()).await.expect("write_new should succeed");

    let stream = store.open(DIRECTORY, "300x450.jpg").await.expect("open should succeed");

    let chunks: Vec<Bytes> = stream.try_collect().await.expect("stream should yield the file");
    assert_eq!(chunks.concat(), b"jpeg-bytes");
}

#[tokio::test]
#[ignore = "touches filesystem"]
async fn delete_directory_removes_all_files() {
    let root = tempfile::tempdir().expect("temp dir");
    let store = FsImageStore::new(root.path());
    store.write_new(DIRECTORY, "300x450.jpg", b"a".to_vec()).await.expect("write_new should succeed");
    store.write_new(DIRECTORY, "100x150.jpg", b"b".to_vec()).await.expect("write_new should succeed");
    store.write_new("other", "300x450.jpg", b"c".to_vec()).await.expect("write_new should succeed");

    store.delete_directory(DIRECTORY).await.expect("delete_directory should succeed");

    assert!(!root.path().join(DIRECTORY).exists());
    assert!(root.path().join("other").join("300x450.jpg").exists());
}
