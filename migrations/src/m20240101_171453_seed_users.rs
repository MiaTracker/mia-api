use sea_orm_migration::prelude::*;
use uuid::Uuid;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let insert = Query::insert()
            .into_table(Users::Table)
            .columns([Users::Uuid, Users::Email, Users::Username, Users::PasswordHash, Users::Admin])
            .values_panic([Uuid::new_v4().into(), "admin@company.com".into(), "Admin".into(), "$argon2id$v=19$m=19456,t=2,p=1$X5xkMjOn6alzVdgZYgqBBQ$29eiouPP56KkjhhbN0vIB3f09kU/iRNBcjZfL6aKIic".into(), true.into()])
            .to_owned();
        manager.exec_stmt(insert).await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let delete = Query::delete()
            .from_table(Users::Table)
            .cond_where(Condition::all().add(Expr::col(Users::Username).eq("Admin")))
            .to_owned();
        manager.exec_stmt(delete).await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Uuid,
    Email,
    Username,
    PasswordHash,
    Admin
}