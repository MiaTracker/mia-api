//! Conversations: create, list, show, delete, add an Exchange and follow it.

use chrono::{DateTime, FixedOffset, Utc};
use entities::sea_orm_active_enums::{ExchangeStatus, MessageRole};
use entities::{conversations, exchanges, messages};
use sea_orm::sea_query::{Expr, Func, Query};
use sea_orm::ActiveValue::{NotSet, Set};
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, DbErr, EntityTrait, ModelTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, SqlErr, TransactionTrait};
use serde_json::json;
use views::conversations::{Conversation, ConversationDetails, ExchangeCreate, ExchangeError, ExchangeEvent, ExchangeStatusView, ExchangeView};
use views::media::PageReq;
use views::users::CurrentUser;
use crate::chat::display::{display_blocks, user_text};
use crate::chat::failure::{exchange_error, ExchangeFailure};
use crate::chat::registry::{ExchangeRegistry, ExchangeSubscription};
use crate::infrastructure::{Clock, RuleViolation, SrvErr};
use crate::jobs::{Job, Jobs};

const IN_PROGRESS_INDEX: &str = "idx_exchanges_one_in_progress";

pub async fn create(user: &CurrentUser, db: &DbConn, clock: &dyn Clock) -> Result<Conversation, SrvErr> {
    let now = clock.now().fixed_offset();
    let model = conversations::ActiveModel {
        user_id: Set(user.id),
        title: NotSet,
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    Ok(conversation_view(model.insert(db).await?))
}

pub async fn index(page: PageReq, user: &CurrentUser, db: &DbConn) -> Result<Vec<Conversation>, SrvErr> {
    let found = conversations::Entity::find()
        .filter(conversations::Column::UserId.eq(user.id))
        .order_by_desc(conversations::Column::UpdatedAt)
        .order_by_desc(conversations::Column::Id)
        .offset(page.offset)
        .limit(page.limit)
        .all(db)
        .await?;
    Ok(found.into_iter().map(conversation_view).collect())
}

pub async fn details(conversation_id: i32, user: &CurrentUser, db: &DbConn) -> Result<ConversationDetails, SrvErr> {
    let conversation = find_conversation(conversation_id, user, db).await?;
    let found_exchanges = exchanges::Entity::find()
        .filter(exchanges::Column::ConversationId.eq(conversation_id))
        .order_by_asc(exchanges::Column::CreatedAt)
        .order_by_asc(exchanges::Column::Id)
        .all(db)
        .await?;
    let exchange_ids: Vec<i32> = found_exchanges.iter().map(|exchange| exchange.id).collect();
    let found_messages = messages::Entity::find()
        .filter(messages::Column::ExchangeId.is_in(exchange_ids))
        .order_by_asc(messages::Column::Id)
        .all(db)
        .await?;

    let exchanges = found_exchanges
        .into_iter()
        .map(|exchange| {
            let own: Vec<messages::Model> = found_messages.iter().filter(|message| message.exchange_id == exchange.id).cloned().collect();
            exchange_view(exchange, &own)
        })
        .collect();
    Ok(ConversationDetails {
        id: conversation.id,
        title: conversation.title,
        created_at: conversation.created_at.with_timezone(&Utc),
        updated_at: conversation.updated_at.with_timezone(&Utc),
        exchanges,
    })
}

/// Deletes the Conversation. Its Exchanges and Messages cascade; LLM calls stay, detached.
pub async fn delete(conversation_id: i32, user: &CurrentUser, db: &DbConn) -> Result<(), SrvErr> {
    let conversation = find_conversation(conversation_id, user, db).await?;
    conversation.delete(db).await?;
    Ok(())
}

/// Adds an Exchange with the user's message to the Conversation and starts running it. The
/// returned subscription already follows the Exchange, so nothing it publishes is missed.
pub async fn start_exchange(conversation_id: i32, params: ExchangeCreate, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry, jobs: &dyn Jobs, clock: &dyn Clock) -> Result<ExchangeSubscription, SrvErr> {
    if params.content.trim().is_empty() {
        return Err(violation(RuleViolation::ExchangeContentEmpty));
    }
    find_conversation(conversation_id, user, db).await?;
    let running = exchanges::Entity::find()
        .filter(exchanges::Column::ConversationId.eq(conversation_id))
        .filter(exchanges::Column::Status.eq(ExchangeStatus::InProgress))
        .count(db)
        .await?;
    if running != 0 {
        return Err(violation(RuleViolation::ConversationExchangeInProgress));
    }

    let now = clock.now().fixed_offset();
    let exchange_id = insert_exchange(conversation_id, &params.content, now, db).await?;

    registry.open(exchange_id);
    let subscription = registry.subscribe(exchange_id)
        .ok_or_else(|| SrvErr::Internal(format!("exchange {exchange_id} is not open right after opening it")))?;
    jobs.submit(Job::RunExchange { exchange_id }).await;
    Ok(subscription)
}

/// Retries a failed Exchange: it goes back to `in_progress`, moves to the end of the Conversation
/// (its `created_at` becomes now), loses its trailing assistant Message if it has one, and runs
/// again from the Messages that are left. The returned subscription replays those Messages' blocks
/// and then follows the run.
///
/// An Exchange whose registry entry is still open (its run has committed the failure but not yet
/// closed the entry) is rejected as in progress: opening an entry over it would let the old run's
/// last publish and close hit the new one.
pub async fn retry_exchange(conversation_id: i32, exchange_id: i32, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry, jobs: &dyn Jobs, clock: &dyn Clock) -> Result<ExchangeSubscription, SrvErr> {
    find_conversation(conversation_id, user, db).await?;
    find_exchange(conversation_id, exchange_id, db).await?;
    if registry.is_open(exchange_id) {
        return Err(violation(RuleViolation::ConversationExchangeInProgress));
    }

    let kept = reopen_exchange(conversation_id, exchange_id, clock.now().fixed_offset(), db).await?;

    registry.open_with(exchange_id, display_blocks(&kept));
    let subscription = registry.subscribe(exchange_id)
        .ok_or_else(|| SrvErr::Internal(format!("exchange {exchange_id} is not open right after opening it")))?;
    jobs.submit(Job::RunExchange { exchange_id }).await;
    Ok(subscription)
}

/// Follows an Exchange: the live stream when it is open in `registry`, otherwise a replay built
/// from the database. An Exchange that is still `in_progress` there gets no terminal event.
pub async fn subscribe(conversation_id: i32, exchange_id: i32, user: &CurrentUser, db: &DbConn, registry: &ExchangeRegistry) -> Result<ExchangeSubscription, SrvErr> {
    find_conversation(conversation_id, user, db).await?;
    find_exchange(conversation_id, exchange_id, db).await?;
    if let Some(subscription) = registry.subscribe(exchange_id) {
        return Ok(subscription);
    }
    // Not open. The registry entry is closed only after the terminal status has committed, so
    // this read cannot be stale; it must come after the registry check.
    let exchange = find_exchange(conversation_id, exchange_id, db).await?;

    let stored = messages::Entity::find()
        .filter(messages::Column::ExchangeId.eq(exchange_id))
        .order_by_asc(messages::Column::Id)
        .all(db)
        .await?;
    let mut replay: Vec<ExchangeEvent> = display_blocks(&stored)
        .into_iter()
        .enumerate()
        .map(|(block, shown)| ExchangeEvent::replay(block as u32, shown))
        .collect();
    match exchange.status {
        ExchangeStatus::Completed => replay.push(ExchangeEvent::Completed { exchange_id }),
        ExchangeStatus::Failed => {
            let error = stored_error(&exchange);
            replay.push(ExchangeEvent::Failed { exchange_id, key: error.key, debug_message: error.debug_message });
        }
        ExchangeStatus::InProgress => {}
    }
    Ok(ExchangeSubscription { replay, live: None })
}

/// True when `err` is a violation of the unique index that allows one in-progress Exchange per Conversation.
pub fn is_exchange_in_progress_conflict(err: &DbErr) -> bool {
    matches!(err.sql_err(), Some(SqlErr::UniqueConstraintViolation(message)) if message.contains(IN_PROGRESS_INDEX))
}

fn violation(violation: RuleViolation) -> SrvErr {
    SrvErr::RuleViolation(vec![violation])
}

/// Inserts the in-progress Exchange, the user's Message and the Conversation's `updated_at` in one transaction.
async fn insert_exchange(conversation_id: i32, content: &str, now: DateTime<FixedOffset>, db: &DbConn) -> Result<i32, SrvErr> {
    let tran = db.begin().await?;
    let exchange = exchanges::ActiveModel {
        conversation_id: Set(conversation_id),
        status: Set(ExchangeStatus::InProgress),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&tran)
    .await
    .map_err(conflict_or_db)?;
    messages::ActiveModel {
        exchange_id: Set(exchange.id),
        role: Set(MessageRole::User),
        content: Set(json!([{"type": "text", "text": content}])),
        created_at: Set(now),
        ..Default::default()
    }
    .insert(&tran)
    .await?;
    conversations::Entity::update_many()
        .set(conversations::ActiveModel { updated_at: Set(now), ..Default::default() })
        .filter(conversations::Column::Id.eq(conversation_id))
        .exec(&tran)
        .await?;
    tran.commit().await?;
    Ok(exchange.id)
}

/// In one transaction: sets the failed Exchange back to `in_progress` at `now`, deletes its last
/// Message if that is an assistant Message, and bumps the Conversation's `updated_at`. Returns the
/// Messages the Exchange keeps, in order. `ExchangeNotFailed` when the Exchange is not failed.
async fn reopen_exchange(conversation_id: i32, exchange_id: i32, now: DateTime<FixedOffset>, db: &DbConn) -> Result<Vec<messages::Model>, SrvErr> {
    let tran = db.begin().await?;
    let reopened = exchanges::Entity::update_many()
        .set(exchanges::ActiveModel {
            status: Set(ExchangeStatus::InProgress),
            error: Set(None),
            completed_at: Set(None),
            created_at: Set(now),
            ..Default::default()
        })
        .filter(exchanges::Column::Id.eq(exchange_id))
        .filter(exchanges::Column::Status.eq(ExchangeStatus::Failed))
        .exec(&tran)
        .await
        .map_err(conflict_or_db)?;
    if reopened.rows_affected == 0 {
        tran.rollback().await?;
        return Err(violation(RuleViolation::ExchangeNotFailed));
    }
    let last_message = Query::select()
        .expr(Func::max(Expr::col(messages::Column::Id)))
        .from(messages::Entity)
        .and_where(messages::Column::ExchangeId.eq(exchange_id))
        .to_owned();
    messages::Entity::delete_many()
        .filter(messages::Column::Role.eq(MessageRole::Assistant))
        .filter(messages::Column::Id.in_subquery(last_message))
        .exec(&tran)
        .await?;
    conversations::Entity::update_many()
        .set(conversations::ActiveModel { updated_at: Set(now), ..Default::default() })
        .filter(conversations::Column::Id.eq(conversation_id))
        .exec(&tran)
        .await?;
    let kept = messages::Entity::find()
        .filter(messages::Column::ExchangeId.eq(exchange_id))
        .order_by_asc(messages::Column::Id)
        .all(&tran)
        .await?;
    tran.commit().await?;
    Ok(kept)
}

fn conflict_or_db(err: DbErr) -> SrvErr {
    if is_exchange_in_progress_conflict(&err) {
        violation(RuleViolation::ConversationExchangeInProgress)
    } else {
        err.into()
    }
}

async fn find_conversation(conversation_id: i32, user: &CurrentUser, db: &DbConn) -> Result<conversations::Model, SrvErr> {
    conversations::Entity::find_by_id(conversation_id)
        .filter(conversations::Column::UserId.eq(user.id))
        .one(db)
        .await?
        .ok_or(SrvErr::NotFound)
}

async fn find_exchange(conversation_id: i32, exchange_id: i32, db: &DbConn) -> Result<exchanges::Model, SrvErr> {
    exchanges::Entity::find_by_id(exchange_id)
        .filter(exchanges::Column::ConversationId.eq(conversation_id))
        .one(db)
        .await?
        .ok_or(SrvErr::NotFound)
}

fn conversation_view(model: conversations::Model) -> Conversation {
    Conversation {
        id: model.id,
        title: model.title,
        created_at: model.created_at.with_timezone(&Utc),
        updated_at: model.updated_at.with_timezone(&Utc),
    }
}

fn exchange_view(exchange: exchanges::Model, stored: &[messages::Model]) -> ExchangeView {
    let content = stored.iter().find(|message| message.role == MessageRole::User).map(user_text).unwrap_or_default();
    let status = match exchange.status {
        ExchangeStatus::InProgress => ExchangeStatusView::InProgress,
        ExchangeStatus::Completed => ExchangeStatusView::Completed,
        ExchangeStatus::Failed => ExchangeStatusView::Failed,
    };
    let error = (exchange.status == ExchangeStatus::Failed).then(|| stored_error(&exchange));
    ExchangeView {
        id: exchange.id,
        status,
        created_at: exchange.created_at.with_timezone(&Utc),
        completed_at: exchange.completed_at.map(|at| at.with_timezone(&Utc)),
        content,
        blocks: display_blocks(stored),
        error,
    }
}

/// The error stored on a failed Exchange; `Internal` when it is missing or unreadable.
fn stored_error(exchange: &exchanges::Model) -> ExchangeError {
    exchange
        .error
        .clone()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_else(|| exchange_error(ExchangeFailure::Internal, "the failed exchange has no readable error".into()))
}

#[cfg(test)]
mod tests {
    use sea_orm::Value;
    use serde_json::json;
    use views::conversations::{DisplayBlock, ToolCallView};
    use super::*;
    use crate::test_support::db::{assert_no_statement_starting_with, assert_statement_count, assert_wrote, updated};
    use crate::test_support::{self, assert_single_violation, fixed_clock, RecordingJobs};
    use crate::test_support::fixtures::{conversation_model, exchange_model, message_model, user};

    fn page() -> PageReq {
        PageReq { offset: None, limit: None }
    }

    fn text(text: &str) -> serde_json::Value {
        json!([{"type": "text", "text": text}])
    }

    #[tokio::test]
    async fn create_sets_timestamps_from_clock() {
        test_support::init();
        let clock = fixed_clock(2026, 3, 4, 5, 6, 7);
        let stored = conversations::Model {
            created_at: clock.now().fixed_offset(),
            updated_at: clock.now().fixed_offset(),
            ..conversation_model(5, 1)
        };
        let db = test_support::db::mock().append_query_results([[stored]]).into_connection();

        let created = create(&user(), &db, &clock).await.ok().expect("create should succeed");

        assert_eq!(created.id, 5);
        assert_eq!(created.created_at, clock.now());
        assert_eq!(created.updated_at, clock.now());
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "conversations""#, &[Value::from(1), Value::from(clock.now().fixed_offset())]);
    }

    #[tokio::test]
    async fn details_returns_not_found_when_conversation_of_other_user() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<conversations::Model>::new()]).into_connection();

        let result = details(3, &user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn details_returns_blocks_and_error_when_exchange_failed() {
        test_support::init();
        let failed = exchanges::Model {
            error: Some(json!({"key": "ProviderUnavailable", "debug_message": "overloaded"})),
            ..exchange_model(7, 3, ExchangeStatus::Failed)
        };
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[exchange_model(6, 3, ExchangeStatus::Completed), failed]])
            .append_query_results([vec![
                message_model(1, 6, MessageRole::User, text("Earlier")),
                message_model(2, 6, MessageRole::Assistant, text("Answer")),
                message_model(3, 7, MessageRole::User, text("Hi")),
                message_model(4, 7, MessageRole::Assistant, json!([{"type": "thinking", "thinking": "hmm", "signature": ""}, {"type": "text", "text": "Hel"}])),
            ]])
            .into_connection();

        let found = details(3, &user(), &db).await.ok().expect("details should succeed");

        assert_eq!(found.id, 3);
        assert_eq!(found.exchanges.len(), 2);
        let first = &found.exchanges[0];
        assert_eq!((first.id, first.status, first.content.as_str()), (6, ExchangeStatusView::Completed, "Earlier"));
        assert_eq!(first.blocks, vec![DisplayBlock::Text { text: "Answer".into() }]);
        assert!(first.error.is_none());
        let second = &found.exchanges[1];
        assert_eq!((second.id, second.status, second.content.as_str()), (7, ExchangeStatusView::Failed, "Hi"));
        assert_eq!(second.blocks, vec![DisplayBlock::Thinking { thinking: "hmm".into() }, DisplayBlock::Text { text: "Hel".into() }]);
        assert_eq!(second.error, Some(ExchangeError { key: "ProviderUnavailable".into(), debug_message: "overloaded".into() }));
    }

    #[tokio::test]
    async fn details_orders_exchanges_by_created_at_then_id() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([Vec::<exchanges::Model>::new()])
            .append_query_results([Vec::<messages::Model>::new()])
            .into_connection();

        details(3, &user(), &db).await.ok().expect("details should succeed");

        let log = test_support::db::log(db);
        let sql = &log.iter().find(|statement| statement.sql.contains(r#"FROM "exchanges""#)).expect("exchanges query").sql;
        assert!(sql.contains(r#"ORDER BY "exchanges"."created_at" ASC, "exchanges"."id" ASC"#), "unexpected SQL: {sql}");
    }

    #[tokio::test]
    async fn index_orders_by_updated_at_desc() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(2, 1), conversation_model(1, 1)]])
            .into_connection();

        let found = index(page(), &user(), &db).await.ok().expect("index should succeed");

        assert_eq!(found.iter().map(|c| c.id).collect::<Vec<_>>(), vec![2, 1]);
        let log = test_support::db::log(db);
        assert_statement_count(&log, 1);
        assert!(log[0].sql.contains(r#"ORDER BY "conversations"."updated_at" DESC"#), "unexpected SQL: {}", log[0].sql);
    }

    #[tokio::test]
    async fn delete_returns_not_found_when_conversation_of_other_user() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<conversations::Model>::new()]).into_connection();

        let result = delete(3, &user(), &db).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
    }

    #[tokio::test]
    async fn start_exchange_fails_with_content_empty_when_content_is_whitespace() {
        test_support::init();
        let db = test_support::db::mock().into_connection();
        let jobs = RecordingJobs::new();

        let result = start_exchange(3, ExchangeCreate { content: " \n\t".into() }, &user(), &db, &ExchangeRegistry::new(), &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::ExchangeContentEmpty));
        assert_statement_count(&test_support::db::log(db), 0);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn start_exchange_fails_with_exchange_in_progress_when_one_is_running() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[crate::test_support::fixtures::count_row(1)]])
            .into_connection();
        let jobs = RecordingJobs::new();

        let result = start_exchange(3, ExchangeCreate { content: "Hi".into() }, &user(), &db, &ExchangeRegistry::new(), &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::ConversationExchangeInProgress));
        assert_statement_count(&test_support::db::log(db), 2);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn start_exchange_submits_run_exchange_job_and_stores_user_message() {
        test_support::init();
        let clock = fixed_clock(2026, 3, 4, 5, 6, 7);
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[crate::test_support::fixtures::count_row(0)]])
            .append_query_results([[exchange_model(9, 3, ExchangeStatus::InProgress)]])
            .append_query_results([[message_model(1, 9, MessageRole::User, text(" Hi "))]])
            .append_exec_results([updated(1)])
            .into_connection();
        let jobs = RecordingJobs::new();
        let registry = ExchangeRegistry::new();

        let subscription = start_exchange(3, ExchangeCreate { content: " Hi ".into() }, &user(), &db, &registry, &jobs, &clock)
            .await.ok().expect("start_exchange should succeed");

        assert!(subscription.replay.is_empty());
        assert!(subscription.live.is_some());
        assert!(matches!(jobs.take().as_slice(), [Job::RunExchange { exchange_id: 9 }]));
        let log = test_support::db::log(db);
        assert_wrote(&log, r#"INSERT INTO "messages""#, &[Value::from(text(" Hi "))]);
        assert_wrote(&log, r#"UPDATE "conversations""#, &[Value::from(clock.now().fixed_offset())]);
    }

    fn failed_exchange() -> exchanges::Model {
        exchanges::Model {
            error: Some(json!({"key": "ProviderUnavailable", "debug_message": "overloaded"})),
            ..exchange_model(7, 3, ExchangeStatus::Failed)
        }
    }

    fn statements_starting_with(log: &[sea_orm::Statement], prefix: &str) -> usize {
        log.iter().filter(|statement| statement.sql.starts_with(prefix)).count()
    }

    #[tokio::test]
    async fn retry_exchange_returns_not_found_when_conversation_of_other_user() {
        test_support::init();
        let db = test_support::db::mock().append_query_results([Vec::<conversations::Model>::new()]).into_connection();
        let jobs = RecordingJobs::new();

        let result = retry_exchange(3, 7, &user(), &db, &ExchangeRegistry::new(), &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 1);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn retry_exchange_returns_not_found_when_exchange_of_other_conversation() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([Vec::<exchanges::Model>::new()])
            .into_connection();
        let jobs = RecordingJobs::new();

        let result = retry_exchange(3, 7, &user(), &db, &ExchangeRegistry::new(), &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 2);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn retry_exchange_fails_with_exchange_in_progress_when_registry_entry_open() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[failed_exchange()]])
            .into_connection();
        let jobs = RecordingJobs::new();
        let registry = ExchangeRegistry::new();
        registry.open(7);

        let result = retry_exchange(3, 7, &user(), &db, &registry, &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::ConversationExchangeInProgress));
        assert_statement_count(&test_support::db::log(db), 2);
        assert!(jobs.take().is_empty());
    }

    #[tokio::test]
    async fn retry_exchange_fails_with_exchange_not_failed_when_update_matches_no_row() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[exchange_model(7, 3, ExchangeStatus::Completed)]])
            .append_exec_results([updated(0)])
            .into_connection();
        let jobs = RecordingJobs::new();
        let registry = ExchangeRegistry::new();

        let result = retry_exchange(3, 7, &user(), &db, &registry, &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert_single_violation(result, |v| matches!(v, RuleViolation::ExchangeNotFailed));
        let log = test_support::db::log(db);
        assert_no_statement_starting_with(&log, r#"DELETE FROM "messages""#);
        assert_no_statement_starting_with(&log, r#"UPDATE "conversations""#);
        assert!(jobs.take().is_empty());
        assert!(!registry.is_open(7));
    }

    #[tokio::test]
    async fn retry_exchange_leaves_registry_closed_and_submits_no_job_when_delete_fails() {
        test_support::init();
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[failed_exchange()]])
            .append_exec_results([updated(1)])
            .append_exec_errors([DbErr::Custom("connection lost".into())])
            .into_connection();
        let jobs = RecordingJobs::new();
        let registry = ExchangeRegistry::new();

        let result = retry_exchange(3, 7, &user(), &db, &registry, &jobs, &fixed_clock(2026, 1, 1, 0, 0, 0)).await;

        assert!(result.is_err());
        assert!(jobs.take().is_empty());
        assert!(!registry.is_open(7));
    }

    /// The mock DB of a successful Retry of Exchange 7: the lookups, the three writes, and the
    /// Messages the Exchange keeps.
    fn retry_db(kept: Vec<messages::Model>) -> sea_orm::DatabaseConnection {
        test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[failed_exchange()]])
            .append_exec_results([updated(1), test_support::db::deleted(), updated(1)])
            .append_query_results([kept])
            .into_connection()
    }

    #[tokio::test]
    async fn retry_exchange_submits_job_and_replays_kept_blocks_when_exchange_failed() {
        test_support::init();
        let clock = fixed_clock(2026, 3, 4, 5, 6, 7);
        let db = retry_db(vec![
            message_model(1, 7, MessageRole::User, text("Hi")),
            message_model(2, 7, MessageRole::Assistant, text("Hel")),
        ]);
        let jobs = RecordingJobs::new();
        let registry = ExchangeRegistry::new();

        let subscription = retry_exchange(3, 7, &user(), &db, &registry, &jobs, &clock).await.ok().expect("retry_exchange should succeed");

        assert_eq!(subscription.replay, vec![ExchangeEvent::TextDelta { block: 0, text: "Hel".into() }]);
        assert!(subscription.live.is_some());
        assert!(matches!(jobs.take().as_slice(), [Job::RunExchange { exchange_id: 7 }]));
        let log = test_support::db::log(db);
        let status = Value::from(sea_orm::ActiveEnum::to_value(&ExchangeStatus::InProgress));
        assert_wrote(&log, r#"UPDATE "exchanges""#, &[status, Value::from(clock.now().fixed_offset())]);
        assert_wrote(&log, r#"UPDATE "conversations""#, &[Value::from(clock.now().fixed_offset())]);
        assert_eq!(statements_starting_with(&log, r#"DELETE FROM "messages""#), 1);
        let role = Value::from(sea_orm::ActiveEnum::to_value(&MessageRole::Assistant));
        assert_wrote(&log, r#"DELETE FROM "messages""#, &[role, Value::from(7)]);
    }

    #[tokio::test]
    async fn retry_exchange_replays_nothing_when_no_blocks_kept() {
        test_support::init();
        let db = retry_db(vec![message_model(1, 7, MessageRole::User, text("Hi"))]);

        let subscription = retry_exchange(3, 7, &user(), &db, &ExchangeRegistry::new(), &RecordingJobs::new(), &fixed_clock(2026, 1, 1, 0, 0, 0))
            .await.ok().expect("retry_exchange should succeed");

        assert!(subscription.replay.is_empty());
        assert!(subscription.live.is_some());
    }

    #[tokio::test]
    async fn subscribe_replays_from_db_with_terminal_event_when_exchange_finished() {
        test_support::init();
        let failed = exchanges::Model {
            error: Some(json!({"key": "Refused", "debug_message": "no"})),
            ..exchange_model(7, 3, ExchangeStatus::Failed)
        };
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[failed.clone()]])
            .append_query_results([[failed]])
            .append_query_results([vec![
                message_model(1, 7, MessageRole::User, text("Hi")),
                message_model(2, 7, MessageRole::Assistant, json!([{"type": "text", "text": "Hel"}, {"type": "text", "text": ""}, {"type": "text", "text": "lo"}])),
            ]])
            .into_connection();

        let subscription = subscribe(3, 7, &user(), &db, &ExchangeRegistry::new()).await.ok().expect("subscribe should succeed");

        assert_eq!(subscription.replay, vec![
            ExchangeEvent::TextDelta { block: 0, text: "Hel".into() },
            ExchangeEvent::TextDelta { block: 1, text: "lo".into() },
            ExchangeEvent::Failed { exchange_id: 7, key: "Refused".into(), debug_message: "no".into() },
        ]);
        assert!(subscription.live.is_none());
    }

    #[tokio::test]
    async fn subscribe_replays_thinking_then_text_when_exchange_finished() {
        test_support::init();
        let completed = exchange_model(7, 3, ExchangeStatus::Completed);
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[completed.clone()]])
            .append_query_results([[completed]])
            .append_query_results([vec![
                message_model(1, 7, MessageRole::User, text("Hi")),
                message_model(2, 7, MessageRole::Assistant, json!([{"type": "thinking", "thinking": "hmm", "signature": ""}, {"type": "text", "text": "Hi"}])),
            ]])
            .into_connection();

        let subscription = subscribe(3, 7, &user(), &db, &ExchangeRegistry::new()).await.ok().expect("subscribe should succeed");

        assert_eq!(subscription.replay, vec![
            ExchangeEvent::ThinkingDelta { block: 0, text: "hmm".into() },
            ExchangeEvent::TextDelta { block: 1, text: "Hi".into() },
            ExchangeEvent::Completed { exchange_id: 7 },
        ]);
    }

    #[tokio::test]
    async fn subscribe_replays_tool_blocks_from_database_when_exchange_finished() {
        test_support::init();
        let completed = exchange_model(7, 3, ExchangeStatus::Completed);
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[completed.clone()]])
            .append_query_results([[completed]])
            .append_query_results([vec![
                message_model(1, 7, MessageRole::User, text("Hi")),
                message_model(2, 7, MessageRole::Assistant, json!([
                    {"type": "thinking", "thinking": "hmm", "signature": ""},
                    {"type": "tool_use", "id": "t1", "name": "mia_search", "input": {"query": "Alien"}}
                ])),
                message_model(3, 7, MessageRole::Tool, json!([
                    {"type": "tool_result", "tool_use_id": "t1", "content": "found", "mia_display": {"name": "mia_search", "query": "Alien", "count": 2, "failed": false}}
                ])),
                message_model(4, 7, MessageRole::Assistant, json!([{"type": "text", "text": "Found it"}])),
            ]])
            .into_connection();

        let subscription = subscribe(3, 7, &user(), &db, &ExchangeRegistry::new()).await.ok().expect("subscribe should succeed");

        let call = ToolCallView { name: "mia_search".into(), query: Some("Alien".into()), media_type: None, count: Some(2), titles: None, failed: false };
        assert_eq!(subscription.replay, vec![
            ExchangeEvent::ThinkingDelta { block: 0, text: "hmm".into() },
            ExchangeEvent::ToolCall { block: 1, call },
            ExchangeEvent::TextDelta { block: 2, text: "Found it".into() },
            ExchangeEvent::Completed { exchange_id: 7 },
        ]);
    }

    #[tokio::test]
    async fn subscribe_returns_not_found_when_exchange_belongs_to_other_conversation() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(7);
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([Vec::<exchanges::Model>::new()])
            .into_connection();

        let result = subscribe(3, 7, &user(), &db, &registry).await;

        assert!(matches!(result, Err(SrvErr::NotFound)));
        assert_statement_count(&test_support::db::log(db), 2);
    }

    #[tokio::test]
    async fn subscribe_uses_registry_when_exchange_is_running() {
        test_support::init();
        let registry = ExchangeRegistry::new();
        registry.open(7);
        registry.publish(7, ExchangeEvent::TextDelta { block: 0, text: "Hel".into() });
        let db = test_support::db::mock()
            .append_query_results([[conversation_model(3, 1)]])
            .append_query_results([[exchange_model(7, 3, ExchangeStatus::InProgress)]])
            .into_connection();

        let subscription = subscribe(3, 7, &user(), &db, &registry).await.ok().expect("subscribe should succeed");

        assert_eq!(subscription.replay, vec![ExchangeEvent::TextDelta { block: 0, text: "Hel".into() }]);
        assert!(subscription.live.is_some());
        assert_statement_count(&test_support::db::log(db), 2);
    }
}
