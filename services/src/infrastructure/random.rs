use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use uuid::Uuid;

/// The source of random values. Services take `&dyn Random` instead of generating UUIDs and
/// salts themselves so tests can make them deterministic.
pub trait Random: Send + Sync {
    fn uuid(&self) -> Uuid;
    fn salt(&self) -> SaltString;
}

/// Randomness from the operating system.
pub struct OsRandom;

impl Random for OsRandom {
    fn uuid(&self) -> Uuid {
        Uuid::new_v4()
    }

    fn salt(&self) -> SaltString {
        SaltString::generate(&mut OsRng)
    }
}
