use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(Table::create()
            .table(Users::Table)
            .col(ColumnDef::new(Users::Id)
                .integer()
                .not_null()
                .primary_key()
                .auto_increment())
            .col(ColumnDef::new(Users::Uuid)
                .uuid()
                .not_null()
                .unique_key())
            .col(ColumnDef::new(Users::Email)
                .string()
                .not_null()
                .unique_key())
            .col(ColumnDef::new(Users::Username)
                .string()
                .not_null()
                .unique_key())
            .col(ColumnDef::new(Users::PasswordHash)
                .string()
                .not_null())
            .col(ColumnDef::new(Users::Admin)
                .boolean()
                .not_null())
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Users::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Uuid,
    Email,
    Username,
    PasswordHash,
    Admin
}
