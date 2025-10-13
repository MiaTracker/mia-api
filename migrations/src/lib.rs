use sea_orm_migration::sea_orm::{ConnectOptions, Database};
pub use sea_orm_migration::prelude::*;
use infrastructure::config;
use infrastructure::fail;

mod m20231004_133637_initial;
mod m20231102_160504_sources;
mod m20231112_145410_add_logs_userid;
mod m20231231_113504_fix_multiuser_support;
mod m20240101_171453_seed_users;
mod m20240106_122113_fix_multiuser_support;
mod m20240106_122631_make_fields_optional;
mod m20240116_180713_sources_name_index;
mod m20240118_185345_add_log_sources;
mod m20240121_170225_rename_log_rating;
mod m20240124_142349_fix_watchlist_table;
mod m20240208_125123_app_tokens;
mod m20240208_152207_add_bot_created;
mod m20240217_103742_app_token_generated;
mod m20240410_130251_cleanup;
mod m20240410_140155_seasons_fix;
mod m20251013_121027_locks;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20231004_133637_initial::Migration),
            Box::new(m20231102_160504_sources::Migration),
            Box::new(m20231112_145410_add_logs_userid::Migration),
            Box::new(m20231231_113504_fix_multiuser_support::Migration),
            Box::new(m20240101_171453_seed_users::Migration),
            Box::new(m20240106_122113_fix_multiuser_support::Migration),
            Box::new(m20240106_122631_make_fields_optional::Migration),
            Box::new(m20240116_180713_sources_name_index::Migration),
            Box::new(m20240118_185345_add_log_sources::Migration),
            Box::new(m20240121_170225_rename_log_rating::Migration),
            Box::new(m20240124_142349_fix_watchlist_table::Migration),
            Box::new(m20240208_125123_app_tokens::Migration),
            Box::new(m20240208_152207_add_bot_created::Migration),
            Box::new(m20240217_103742_app_token_generated::Migration),
            Box::new(m20240410_130251_cleanup::Migration),
            Box::new(m20240410_140155_seasons_fix::Migration),
            Box::new(m20251013_121027_locks::Migration),
        ]
    }
}

macro_rules! prepare_connection {
    () => {
        {
            let connect_options = ConnectOptions::new(&infrastructure::config().db.connection_url)
                .set_schema_search_path(&config().db.schema)
                .to_owned();
            let db = match Database::connect(connect_options).await {
                Ok(res) => { res }
                Err(err) => {
                    fail!("Failed to connect to database using connection string \"{}\". Reason: {}", &config().db.connection_url, err);
                }
            };
            db
        }
    };
}

pub async fn migrate_up(num: Option<u32>) {
    let db = prepare_connection!();
    match Migrator::up(&db, num).await {
        Ok(_) => { println!("Successfully applied migrations") }
        Err(err) => {
            fail!("Failed to apply migrations. Reason: {}", err);
        }
    }
}

pub async fn migrate_down(num: Option<u32>) {
    let db = prepare_connection!();
    match Migrator::down(&db, num).await {
        Ok(_) => { println!("Successfully rolled back") }
        Err(err) => {
            fail!("Failed to rollback. Reason: {}", err);
        }
    }
}


pub async fn migrate_status() {
    let db = prepare_connection!();
    match Migrator::status(&db).await {
        Ok(_) => { }
        Err(err) => {
            fail!("Failed check status. Reason: {}", err);
        }
    }
}

pub async fn migrate_fresh() {
    let db = prepare_connection!();
    match Migrator::fresh(&db).await {
        Ok(_) => { println!("Successfully reapplied all migrations") }
        Err(err) => {
            fail!("Failed to reapply migrations. Reason: {}", err);
        }
    }
}

pub async fn migrate_refresh() {
    let db = prepare_connection!();
    match Migrator::refresh(&db).await {
        Ok(_) => { println!("Successfully refreshed all migrations") }
        Err(err) => {
            fail!("Failed to refresh migrations. Reason: {}", err);
        }
    }
}

pub async fn migrate_reset() {
    let db = prepare_connection!();
    match Migrator::reset(&db).await {
        Ok(_) => { println!("Successfully reset the database") }
        Err(err) => {
            fail!("Failed to reset the database. Reason: {}", err);
        }
    }
}