//! The chat behaviour a mock database cannot prove: the partial unique index's error, the bulk
//! recovery SQL, rows vanishing under a running Exchange, and one run from start to stored answer.
//! Needs Docker: `cargo test -p services --test postgres_chat -- --ignored`.

mod common;

use std::sync::Arc;
use common::postgres::database;
use common::seed;
use entities::prelude::{LlmCalls, Messages};
use entities::sea_orm_active_enums::{ExchangeStatus, MessageRole};
use entities::{conversations, exchanges, llm_calls, messages};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use integrations::openrouter::{BlockKind, LlmEvent, LlmUsage, StopReason};
use sea_orm::{ActiveModelTrait, DbConn, EntityTrait, NotSet, QueryOrder, Set};
use serde_json::json;
use services::chat::{self, ExchangeRegistry};
use services::jobs::JobDeps;
use services::test_support::llm::{text_answer, with_answer, with_stream};
use services::test_support::{fixed_clock, InlineJobs, MockLlm};
use views::conversations::{DisplayBlock, ExchangeCreate, ExchangeEvent, ExchangeStatusView};

#[tokio::test]
#[ignore = "requires docker"]
async fn is_exchange_in_progress_conflict_returns_true_when_partial_unique_index_violated() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    seed::exchange(&db, conversation.id, ExchangeStatus::InProgress).await;

    let err = exchanges::ActiveModel {
        id: NotSet,
        conversation_id: Set(conversation.id),
        status: Set(ExchangeStatus::InProgress),
        error: Set(None),
        created_at: Set(seed::chat_time()),
        completed_at: Set(None),
    }.insert(&db).await.expect_err("a second in-progress Exchange is rejected");

    assert!(chat::is_exchange_in_progress_conflict(&err), "{err}");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn recover_interrupted_marks_exchange_failed_and_closes_open_llm_call() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let interrupted = seed::conversation(&db, &user).await;
    let finished = seed::conversation(&db, &user).await;
    let running = seed::exchange(&db, interrupted.id, ExchangeStatus::InProgress).await;
    let completed = seed::exchange(&db, finished.id, ExchangeStatus::Completed).await;
    let open_call = seed::llm_call(&db, &user, running.id).await;
    let completed_call = seed::llm_call(&db, &user, completed.id).await;
    let clock = fixed_clock(2024, 1, 2, 8, 0, 0);

    let marked = chat::recover_interrupted(&db, &clock).await.ok().expect("recovering");

    assert_eq!(marked, 1);
    let running_after = exchange(&db, running.id).await;
    assert_eq!(running_after.status, ExchangeStatus::Failed);
    assert_eq!(running_after.error.expect("exchange error")["key"], "Interrupted");
    assert_eq!(running_after.completed_at, Some(clock_time(&clock)));
    let call_after = llm_call(&db, open_call.id).await;
    assert_eq!(call_after.error, Some(json!({"message": "interrupted"})));
    assert_eq!(call_after.completed_at, Some(clock_time(&clock)));
    assert_eq!(exchange(&db, completed.id).await, completed, "the completed Exchange is untouched");
    assert_eq!(llm_call(&db, completed_call.id).await, completed_call, "its LLM call is untouched");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn run_exchange_stops_quietly_when_conversation_deleted_mid_exchange() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let running = seed::exchange(&db, conversation.id, ExchangeStatus::InProgress).await;
    seed::user_message(&db, running.id, "Hi").await;
    let registry = ExchangeRegistry::new();
    registry.open(running.id);
    let subscription = registry.subscribe(running.id).expect("open Exchange");
    // Yields the start of an answer, deletes the Conversation, then yields the rest of it.
    let deleting = futures::stream::unfold((0, db.clone(), conversation.id), |(step, db, conversation_id)| async move {
        let event = match step {
            0 => LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() },
            1 => LlmEvent::BlockStart { index: 0, kind: BlockKind::Text },
            2 => LlmEvent::TextDelta { index: 0, text: "Hel".into() },
            3 => {
                conversations::Entity::delete_by_id(conversation_id).exec(&*db).await.expect("deleting the Conversation");
                LlmEvent::TextDelta { index: 0, text: "lo".into() }
            }
            4 => LlmEvent::BlockStop { index: 0 },
            5 => LlmEvent::MessageDelta { stop_reason: Some(StopReason::EndTurn), usage: LlmUsage::default() },
            6 => LlmEvent::MessageStop,
            _ => return None,
        };
        Some((Ok(event), (step + 1, db, conversation_id)))
    });

    let result = chat::run_exchange(running.id, &db, &with_stream(deleting.boxed()), &registry, &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

    assert!(result.is_ok());
    let calls = LlmCalls::find().all(&*db).await.expect("querying LLM calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].exchange_id, None);
    assert_eq!(calls[0].error, Some(json!({"message": "conversation deleted"})));
    assert!(calls[0].completed_at.is_some());
    let events = drain(subscription.live.expect("live")).await;
    assert!(events.iter().all(|event| matches!(event, ExchangeEvent::TextDelta { .. })), "no terminal event: {events:?}");
}

#[tokio::test]
#[ignore = "requires docker"]
async fn start_exchange_persists_answer_and_ledger_when_stream_completes() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 1, 12, 0, 0));
    let llm: MockLlm = with_answer(text_answer(&["Hello", ", world"], StopReason::EndTurn));
    let jobs = InlineJobs::new(JobDeps {
        db: db.clone(),
        store: Arc::new(services::test_support::InMemoryImageStore::new()),
        llm: Arc::new(llm),
        registry: registry.clone(),
        clock: clock.clone(),
    });

    let subscription = chat::start_exchange(conversation.id, ExchangeCreate { content: "Hi".into() }, &user, &db, &registry, &jobs, &*clock)
        .await.ok().expect("starting the Exchange");

    let stored = Messages::find().order_by_asc(messages::Column::Id).all(&*db).await.expect("querying messages");
    assert_eq!(stored.iter().map(|message| message.role.clone()).collect::<Vec<_>>(), vec![MessageRole::User, MessageRole::Assistant]);
    assert_eq!(stored[0].content, json!([{"type": "text", "text": "Hi"}]));
    assert_eq!(stored[1].content, json!([{"type": "text", "text": "Hello, world"}]));
    let calls = LlmCalls::find().all(&*db).await.expect("querying LLM calls");
    assert_eq!(calls.len(), 1);
    assert_eq!(stored[1].llm_call_id, Some(calls[0].id));
    assert_eq!((calls[0].input_tokens, calls[0].output_tokens), (Some(10), Some(5)));
    assert_eq!(calls[0].stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(calls[0].error, None);
    assert_eq!(calls[0].completed_at, Some(seed::chat_time()));
    let details = chat::details(conversation.id, &user, &db).await.ok().expect("details");
    assert_eq!(details.exchanges.len(), 1);
    assert_eq!(details.exchanges[0].status, ExchangeStatusView::Completed);
    assert_eq!(details.exchanges[0].blocks, vec![DisplayBlock::Text { text: "Hello, world".into() }]);
    let events: Vec<_> = subscription.replay.into_iter().chain(drain(subscription.live.expect("live")).await).collect();
    assert_eq!(events.last(), Some(&ExchangeEvent::Completed { exchange_id: details.exchanges[0].id }));
}

/// Every event until the registry closes the Exchange.
async fn drain(mut live: UnboundedReceiver<ExchangeEvent>) -> Vec<ExchangeEvent> {
    let mut events = Vec::new();
    while let Some(event) = live.recv().await {
        events.push(event);
    }
    events
}

async fn exchange(db: &DbConn, id: i32) -> exchanges::Model {
    exchanges::Entity::find_by_id(id).one(db).await.expect("querying exchange").expect("exchange row")
}

async fn llm_call(db: &DbConn, id: i32) -> llm_calls::Model {
    llm_calls::Entity::find_by_id(id).one(db).await.expect("querying LLM call").expect("LLM call row")
}

fn clock_time(clock: &services::test_support::FixedClock) -> chrono::DateTime<chrono::FixedOffset> {
    use services::infrastructure::Clock;
    clock.now().fixed_offset()
}
