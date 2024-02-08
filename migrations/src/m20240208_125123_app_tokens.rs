use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_table(Table::create()
            .table(AppTokens::Table)
            .col(ColumnDef::new(AppTokens::Uuid).uuid().not_null().primary_key())
            .col(ColumnDef::new(AppTokens::Name).string().not_null())
            .col(ColumnDef::new(AppTokens::UserId).integer().not_null())
            .foreign_key(ForeignKey::create().name("user_id_fk")
                .from(AppTokens::Table, AppTokens::UserId)
                .to(Users::Table, Users::Id)
                .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
            .index(Index::create().name("name_uniq_const")
                .col(AppTokens::UserId).col(AppTokens::Name).unique())
            .to_owned()
        ).await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(AppTokens::Table).to_owned()).await
    }
}

#[derive(DeriveIden)]
enum AppTokens {
    Table,
    Uuid,
    Name,
    UserId,
}


#[derive(DeriveIden)]
enum Users {
    Table,
    Id
}
