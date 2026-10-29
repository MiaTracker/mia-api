//! Lowers the process-wide file size limit (`RLIMIT_FSIZE`) for the lifetime of a guard. Linux only.
//!
//! The limit applies to every file the process writes, including the test harness's stdout when it
//! is redirected to a file, so a binary using this must hold exactly one test, and the guard must
//! be dropped before the test's assertions run.

pub struct FileSizeLimit {
    previous: libc::rlimit,
}

impl FileSizeLimit {
    /// Writing past `bytes` fails with EFBIG; SIGXFSZ (which would kill the process) is ignored.
    pub fn new(bytes: u64) -> Self {
        // SAFETY: plain libc calls with valid pointers; they affect this process only.
        unsafe {
            assert_ne!(libc::signal(libc::SIGXFSZ, libc::SIG_IGN), libc::SIG_ERR, "ignoring SIGXFSZ");
            let mut previous = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(libc::getrlimit(libc::RLIMIT_FSIZE, &mut previous), 0, "reading RLIMIT_FSIZE");
            let limit = libc::rlimit { rlim_cur: bytes, rlim_max: previous.rlim_max };
            assert_eq!(libc::setrlimit(libc::RLIMIT_FSIZE, &limit), 0, "setting RLIMIT_FSIZE");
            Self { previous }
        }
    }
}

impl Drop for FileSizeLimit {
    fn drop(&mut self) {
        // SAFETY: restores the limit read in `new`; raising the soft limit up to the unchanged hard limit is allowed.
        unsafe {
            libc::setrlimit(libc::RLIMIT_FSIZE, &self.previous);
        }
    }
}
