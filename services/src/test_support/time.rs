use std::sync::Mutex;
use argon2::password_hash::SaltString;
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use uuid::Uuid;
use crate::infrastructure::{Clock, Random};

/// A clock that only moves when the test says so.
pub struct FixedClock {
    now: Mutex<DateTime<Utc>>,
}

impl FixedClock {
    pub fn new(t: DateTime<Utc>) -> Self {
        Self { now: Mutex::new(t) }
    }

    pub fn set(&self, t: DateTime<Utc>) {
        *self.now.lock().expect("clock lock") = t;
    }

    pub fn advance(&self, d: Duration) {
        *self.now.lock().expect("clock lock") += d;
    }
}

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().expect("clock lock")
    }
}

struct SeqState {
    preset: Vec<Uuid>,
    generated: u128,
}

/// Deterministic randomness: preset UUIDs first, then `Uuid::from_u128(1000 + n)` for the n-th
/// further call. The salt is always the same.
pub struct SeqRandom {
    state: Mutex<SeqState>,
}

impl SeqRandom {
    pub fn new() -> Self {
        Self::with_uuids(Vec::new())
    }

    pub fn with_uuids(mut uuids: Vec<Uuid>) -> Self {
        uuids.reverse();
        Self { state: Mutex::new(SeqState { preset: uuids, generated: 0 }) }
    }
}

impl Default for SeqRandom {
    fn default() -> Self {
        Self::new()
    }
}

impl Random for SeqRandom {
    fn uuid(&self) -> Uuid {
        let mut state = self.state.lock().expect("random lock");
        if let Some(uuid) = state.preset.pop() {
            return uuid;
        }
        let uuid = Uuid::from_u128(1000 + state.generated);
        state.generated += 1;
        uuid
    }

    fn salt(&self) -> SaltString {
        SaltString::from_b64("dGVzdHNhbHR0ZXN0c2FsdA").expect("valid salt")
    }
}

/// A calendar date; panics if it does not exist.
pub fn date(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
}

/// A `FixedClock` set to the given UTC date and time; panics if it does not exist.
pub fn fixed_clock(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> FixedClock {
    FixedClock::new(Utc.with_ymd_and_hms(year, month, day, hour, minute, second).single().expect("valid time"))
}
