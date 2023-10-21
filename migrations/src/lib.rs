pub use sea_orm_migration::prelude::*;

mod m20231004_133637_v0_1_0;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20231004_133637_v0_1_0::Migration),
        ]
    }
}
