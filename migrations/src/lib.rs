pub use sea_orm_migration::prelude::*;

mod m20231004_133637_initial;
mod m20231102_160504_sources;
mod m20231112_145410_add_logs_userid;
mod m20231231_113504_fix_multiuser_support;
mod m20240101_171453_seed_users;
mod m20240106_122113_fix_multiuser_support;
mod m20240106_122631_make_fields_optional;

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
        ]
    }
}
