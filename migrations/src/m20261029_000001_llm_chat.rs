use sea_orm_migration::prelude::*;
use crate::extension::postgres::Type;
use crate::sea_orm::EnumIter;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.create_type(
            Type::create()
                .as_enum(MessageRole::Table)
                .values([MessageRole::User, MessageRole::Tool, MessageRole::Assistant])
                .to_owned()
        ).await?;

        manager.create_type(
            Type::create()
                .as_enum(ExchangeStatus::Table)
                .values([ExchangeStatus::InProgress, ExchangeStatus::Completed, ExchangeStatus::Failed])
                .to_owned()
        ).await?;

        manager.create_type(
            Type::create()
                .as_enum(LlmCallPurpose::Table)
                .values([LlmCallPurpose::Exchange, LlmCallPurpose::Title])
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(Conversations::Table)
                .col(ColumnDef::new(Conversations::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(Conversations::UserId).integer().not_null())
                .col(ColumnDef::new(Conversations::Title).string())
                .col(ColumnDef::new(Conversations::CreatedAt).timestamp_with_time_zone().not_null())
                .col(ColumnDef::new(Conversations::UpdatedAt).timestamp_with_time_zone().not_null())
                .foreign_key(ForeignKey::create()
                    .from(Conversations::Table, Conversations::UserId)
                    .to(Users::Table, Users::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create()
                .name("idx_conversations_user_id_updated_at")
                .table(Conversations::Table)
                .col(Conversations::UserId)
                .col((Conversations::UpdatedAt, IndexOrder::Desc))
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(Exchanges::Table)
                .col(ColumnDef::new(Exchanges::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(Exchanges::ConversationId).integer().not_null())
                .col(ColumnDef::new(Exchanges::Status).enumeration(ExchangeStatus::Table, [ExchangeStatus::InProgress, ExchangeStatus::Completed, ExchangeStatus::Failed]).not_null())
                .col(ColumnDef::new(Exchanges::Error).json_binary())
                .col(ColumnDef::new(Exchanges::CreatedAt).timestamp_with_time_zone().not_null())
                .col(ColumnDef::new(Exchanges::CompletedAt).timestamp_with_time_zone())
                .foreign_key(ForeignKey::create()
                    .from(Exchanges::Table, Exchanges::ConversationId)
                    .to(Conversations::Table, Conversations::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create()
                .name("idx_exchanges_conversation_id")
                .table(Exchanges::Table)
                .col(Exchanges::ConversationId)
                .to_owned()
        ).await?;

        // At most one exchange per conversation may be running at a time
        manager.get_connection().execute_unprepared(
            "CREATE UNIQUE INDEX idx_exchanges_one_in_progress ON exchanges (conversation_id) WHERE status = 'in_progress';"
        ).await?;

        // Ledger of every request made to the LLM provider; rows are never deleted
        manager.create_table(
            Table::create()
                .table(LlmCalls::Table)
                .col(ColumnDef::new(LlmCalls::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(LlmCalls::UserId).integer())
                .col(ColumnDef::new(LlmCalls::ConversationId).integer())
                .col(ColumnDef::new(LlmCalls::ExchangeId).integer())
                .col(ColumnDef::new(LlmCalls::Purpose).enumeration(LlmCallPurpose::Table, [LlmCallPurpose::Exchange, LlmCallPurpose::Title]).not_null())
                .col(ColumnDef::new(LlmCalls::Model).string().not_null())
                .col(ColumnDef::new(LlmCalls::StopReason).string())
                .col(ColumnDef::new(LlmCalls::InputTokens).integer())
                .col(ColumnDef::new(LlmCalls::OutputTokens).integer())
                .col(ColumnDef::new(LlmCalls::CacheReadTokens).integer())
                .col(ColumnDef::new(LlmCalls::CacheCreationTokens).integer())
                .col(ColumnDef::new(LlmCalls::ProviderRequestId).string())
                .col(ColumnDef::new(LlmCalls::Error).json_binary())
                .col(ColumnDef::new(LlmCalls::CreatedAt).timestamp_with_time_zone().not_null())
                .col(ColumnDef::new(LlmCalls::CompletedAt).timestamp_with_time_zone())
                .foreign_key(ForeignKey::create()
                    .from(LlmCalls::Table, LlmCalls::UserId)
                    .to(Users::Table, Users::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::SetNull))
                .foreign_key(ForeignKey::create()
                    .from(LlmCalls::Table, LlmCalls::ConversationId)
                    .to(Conversations::Table, Conversations::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::SetNull))
                .foreign_key(ForeignKey::create()
                    .from(LlmCalls::Table, LlmCalls::ExchangeId)
                    .to(Exchanges::Table, Exchanges::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::SetNull))
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create()
                .name("idx_llm_calls_user_id")
                .table(LlmCalls::Table)
                .col(LlmCalls::UserId)
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create()
                .name("idx_llm_calls_exchange_id")
                .table(LlmCalls::Table)
                .col(LlmCalls::ExchangeId)
                .to_owned()
        ).await?;

        manager.create_table(
            Table::create()
                .table(Messages::Table)
                .col(ColumnDef::new(Messages::Id).integer().primary_key().auto_increment())
                .col(ColumnDef::new(Messages::ExchangeId).integer().not_null())
                .col(ColumnDef::new(Messages::Role).enumeration(MessageRole::Table, [MessageRole::User, MessageRole::Tool, MessageRole::Assistant]).not_null())
                .col(ColumnDef::new(Messages::Content).json_binary().not_null())
                .col(ColumnDef::new(Messages::LlmCallId).integer().unique_key())
                .col(ColumnDef::new(Messages::CreatedAt).timestamp_with_time_zone().not_null())
                .foreign_key(ForeignKey::create()
                    .from(Messages::Table, Messages::ExchangeId)
                    .to(Exchanges::Table, Exchanges::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Cascade))
                .foreign_key(ForeignKey::create()
                    .from(Messages::Table, Messages::LlmCallId)
                    .to(LlmCalls::Table, LlmCalls::Id)
                    .on_update(ForeignKeyAction::Cascade).on_delete(ForeignKeyAction::Restrict))
                .to_owned()
        ).await?;

        manager.create_index(
            Index::create()
                .name("idx_messages_exchange_id")
                .table(Messages::Table)
                .col(Messages::ExchangeId)
                .to_owned()
        ).await?;

        // Exactly the assistant messages are produced by an LLM call
        manager.get_connection().execute_unprepared(
            "ALTER TABLE messages ADD CONSTRAINT chk_messages_llm_call_iff_assistant CHECK ((role = 'assistant') = (llm_call_id IS NOT NULL));"
        ).await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager.drop_table(Table::drop().table(Messages::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(LlmCalls::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Exchanges::Table).to_owned()).await?;
        manager.drop_table(Table::drop().table(Conversations::Table).to_owned()).await?;

        manager.drop_type(Type::drop().name(LlmCallPurpose::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(ExchangeStatus::Table).to_owned()).await?;
        manager.drop_type(Type::drop().name(MessageRole::Table).to_owned()).await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Conversations {
    Table,
    Id,
    UserId,
    Title,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Exchanges {
    Table,
    Id,
    ConversationId,
    Status,
    Error,
    CreatedAt,
    CompletedAt,
}

#[derive(DeriveIden)]
enum LlmCalls {
    Table,
    Id,
    UserId,
    ConversationId,
    ExchangeId,
    Purpose,
    Model,
    StopReason,
    InputTokens,
    OutputTokens,
    CacheReadTokens,
    CacheCreationTokens,
    ProviderRequestId,
    Error,
    CreatedAt,
    CompletedAt,
}

#[derive(DeriveIden)]
enum Messages {
    Table,
    Id,
    ExchangeId,
    Role,
    Content,
    LlmCallId,
    CreatedAt,
}

#[derive(Iden, EnumIter)]
enum MessageRole {
    Table,
    User,
    Tool,
    Assistant,
}

#[derive(Iden, EnumIter)]
enum ExchangeStatus {
    Table,
    InProgress,
    Completed,
    Failed,
}

#[derive(Iden, EnumIter)]
enum LlmCallPurpose {
    Table,
    Exchange,
    Title,
}
