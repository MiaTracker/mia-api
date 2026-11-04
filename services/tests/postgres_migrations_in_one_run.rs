//! `cargo run -- migrate up` on an empty database: every pending migration in one
//! `Migrator::up` call. A binary of its own, because the global config must point at the empty
//! database being migrated (`m20260417_000002_add_still_image_type` connects through it).
//! Needs Docker: `cargo test -p services --test postgres_migrations_in_one_run -- --ignored`.

mod common;

use common::postgres::configured_empty_database;
use migration::{Migrator, MigratorTrait};

#[tokio::test]
#[ignore = "requires docker"]
async fn migrator_up_applies_all_migrations_when_database_is_empty() {
    let db = configured_empty_database().await;

    let result = Migrator::up(&db, None).await;

    assert!(result.is_ok(), "migrating failed: {:?}", result.err());
    assert!(Migrator::get_pending_migrations(&db).await.expect("pending migrations").is_empty());
}
