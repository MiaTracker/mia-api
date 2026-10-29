//! A failed write must not leave a partial file behind. It has its own test binary, holding a
//! single test, because it lowers the process-wide file size limit (see `common/file_size_limit.rs`).
//! Linux only. Run it with `cargo test -p services --test fs_image_store_partial_write -- --ignored`.

#[path = "common/file_size_limit.rs"]
mod file_size_limit;

use file_size_limit::FileSizeLimit;
use services::infrastructure::{FsImageStore, ImageStore};

const DIRECTORY: &str = "0123456789abcdef0123456789abcdef";

/// Larger than tokio's write buffer (2 MiB), so `write_all` itself sees the failure of the first chunk.
const LARGER_THAN_WRITE_BUFFER: usize = 3 * 1024 * 1024;

#[tokio::test]
#[ignore = "touches filesystem"]
async fn write_new_removes_partial_file_when_write_all_fails() {
    let root = tempfile::tempdir().expect("temp dir");
    let store = FsImageStore::new(root.path());

    let limit = FileSizeLimit::new(4);
    let result = store.write_new(DIRECTORY, "300x450.jpg", vec![0u8; LARGER_THAN_WRITE_BUFFER]).await;
    drop(limit);

    assert!(result.is_err(), "writing past the file size limit should fail");
    assert!(!root.path().join(DIRECTORY).join("300x450.jpg").exists(), "the partial file should be removed");
}
