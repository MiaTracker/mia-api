use chrono::{DateTime, Utc};

/// The source of the current time. Services take `&dyn Clock` instead of calling `Utc::now()`
/// so tests can fix the time.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// The wall clock.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}
