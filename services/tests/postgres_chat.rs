//! The chat behaviour a mock database cannot prove: the partial unique index's error, the bulk
//! recovery SQL, rows vanishing under a running Exchange, and one run from start to stored answer.
//! Needs Docker: `cargo test -p services --test postgres_chat -- --ignored`.

mod common;

use std::sync::Arc;
use common::postgres::database;
use common::seed;
use entities::prelude::{LlmCalls, Messages};
use entities::sea_orm_active_enums::{ExchangeStatus, LlmCallPurpose, MessageRole};
use entities::{conversations, exchanges, llm_calls, messages};
use futures::StreamExt;
use tokio::sync::mpsc::UnboundedReceiver;
use integrations::openrouter::{BlockKind, LlmEvent, LlmUsage, StopReason};
use sea_orm::{ActiveModelTrait, ColumnTrait, DbConn, EntityTrait, NotSet, QueryFilter, QueryOrder, Set};
use serde_json::json;
use services::chat::{self, ExchangeRegistry};
use services::jobs::JobDeps;
use services::test_support::llm::{text_answer, with_answer, with_recorded_answers, with_stream};
use services::test_support::{fixed_clock, InlineJobs, MockLlm, MockTmdb};
use services::infrastructure::RuleViolation;
use views::users::CurrentUser;
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

    let marked = chat::recover_interrupted(&db, &clock).await.expect("recovering");

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
async fn recover_interrupted_closes_open_title_call_when_server_stopped_during_title() {
    let db = database().await;
    let user = seed::user(&db, 1).await;
    let titling = seed::conversation(&db, &user).await;
    let titled = seed::conversation(&db, &user).await;
    let finished = seed::exchange(&db, titled.id, ExchangeStatus::Completed).await;
    let open_title = seed::title_call(&db, &user, titling.id, None).await;
    let completed_title = seed::title_call(&db, &user, titled.id, Some(seed::chat_time())).await;
    let exchange_call = seed::llm_call(&db, &user, finished.id).await;
    let clock = fixed_clock(2024, 1, 2, 8, 0, 0);

    let marked = chat::recover_interrupted(&db, &clock).await.expect("recovering");

    assert_eq!(marked, 0);
    let title_after = llm_call(&db, open_title.id).await;
    assert_eq!(title_after.error, Some(json!({"message": "interrupted"})));
    assert_eq!(title_after.completed_at, Some(clock_time(&clock)));
    assert_eq!(llm_call(&db, completed_title.id).await, completed_title, "the completed title call is untouched");
    assert_eq!(llm_call(&db, exchange_call.id).await, exchange_call, "a call of a finished Exchange is untouched");
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

    let result = chat::run_exchange(running.id, &db, &with_stream(deleting.boxed()), &registry, &MockTmdb::new(), &fixed_clock(2024, 1, 1, 12, 0, 0)).await;

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
        title_llm: None,
        registry: registry.clone(),
        tmdb: Arc::new(MockTmdb::new()),
        clock: clock.clone(),
    });

    let subscription = chat::start_exchange(conversation.id, ExchangeCreate { content: "Hi".into() }, &user, &db, &registry, &jobs, &*clock)
        .await.expect("starting the Exchange");

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
    let details = chat::details(conversation.id, &user, &db).await.expect("details");
    assert_eq!(details.exchanges.len(), 1);
    assert_eq!(details.exchanges[0].status, ExchangeStatusView::Completed);
    assert_eq!(details.exchanges[0].blocks, vec![DisplayBlock::Text { text: "Hello, world".into() }]);
    let events: Vec<_> = subscription.replay.into_iter().chain(drain(subscription.live.expect("live")).await).collect();
    assert_eq!(events.last(), Some(&ExchangeEvent::Completed { exchange_id: details.exchanges[0].id }));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn create_persists_conversation_exchange_and_message_when_content_given() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 1, 12, 0, 0));
    let jobs = InlineJobs::new(JobDeps {
        db: db.clone(),
        store: Arc::new(services::test_support::InMemoryImageStore::new()),
        llm: Arc::new(with_answer(text_answer(&["Hello", ", world"], StopReason::EndTurn))),
        title_llm: Some(Arc::new(with_answer(text_answer(&["Alien rewatch"], StopReason::EndTurn)))),
        registry: registry.clone(),
        tmdb: Arc::new(MockTmdb::new()),
        clock: clock.clone(),
    });

    let subscription = chat::create(ExchangeCreate { content: "Hi".into() }, &user, &db, &registry, &jobs, &*clock)
        .await.expect("creating the Conversation");

    let conversations = conversations::Entity::find().all(&*db).await.expect("querying conversations");
    assert_eq!(conversations.len(), 1);
    let conversation = &conversations[0];
    assert_eq!(conversation.user_id, user.id);
    assert_eq!(conversation.title.as_deref(), Some("Alien rewatch"));
    assert_eq!((conversation.created_at, conversation.updated_at), (clock_time(&clock), clock_time(&clock)));
    let details = chat::details(conversation.id, &user, &db).await.expect("details");
    assert_eq!(details.exchanges.len(), 1);
    let exchange_id = details.exchanges[0].id;
    assert_eq!(details.exchanges[0].status, ExchangeStatusView::Completed);
    assert_eq!(exchange(&db, exchange_id).await.created_at, clock_time(&clock));
    let stored = messages_of(&db, exchange_id).await;
    assert_eq!(stored.iter().map(|message| message.role.clone()).collect::<Vec<_>>(), vec![MessageRole::User, MessageRole::Assistant]);
    assert_eq!(stored[0].content, text("Hi"));
    let title_calls = LlmCalls::find().filter(llm_calls::Column::Purpose.eq(LlmCallPurpose::Title)).all(&*db).await.expect("querying LLM calls");
    assert_eq!(title_calls.len(), 1);
    assert_eq!(title_calls[0].exchange_id, None);
    assert_eq!(title_calls[0].conversation_id, Some(conversation.id));
    let events: Vec<ExchangeEvent> = subscription.into_events().collect().await;
    assert_eq!(events, vec![
        ExchangeEvent::ConversationCreated { conversation_id: conversation.id, exchange_id },
        ExchangeEvent::TextDelta { block: 0, text: "Hello, world".into() },
        ExchangeEvent::Completed { exchange_id },
        ExchangeEvent::Title { title: "Alien rewatch".into() },
        ExchangeEvent::StreamEnd {},
    ]);
}

/// Jobs that run `RunExchange` on the spot against `db`, answering with `llm`.
fn inline_jobs(db: &Arc<DbConn>, registry: &Arc<ExchangeRegistry>, clock: &Arc<services::test_support::FixedClock>, llm: MockLlm) -> InlineJobs {
    InlineJobs::new(JobDeps {
        db: db.clone(),
        store: Arc::new(services::test_support::InMemoryImageStore::new()),
        llm: Arc::new(llm),
        title_llm: None,
        registry: registry.clone(),
        tmdb: Arc::new(MockTmdb::new()),
        clock: clock.clone(),
    })
}

fn text(text: &str) -> serde_json::Value {
    json!([{"type": "text", "text": text}])
}

fn tool_use(id: &str) -> serde_json::Value {
    json!([{"type": "tool_use", "id": id, "name": "date_time", "input": {}}])
}

async fn messages_of(db: &DbConn, exchange_id: i32) -> Vec<messages::Model> {
    Messages::find().filter(messages::Column::ExchangeId.eq(exchange_id)).order_by_asc(messages::Column::Id).all(db).await.expect("querying messages")
}

/// A failed Exchange of the Conversation holding a user Message, and the Exchange's LLM call.
async fn failed_exchange(db: &DbConn, user: &CurrentUser, conversation: &conversations::Model) -> (exchanges::Model, llm_calls::Model) {
    let failed = seed::exchange(db, conversation.id, ExchangeStatus::Failed).await;
    seed::user_message(db, failed.id, "Question A").await;
    let call = seed::llm_call(db, user, failed.id).await;
    (failed, call)
}

#[tokio::test]
#[ignore = "requires docker"]
async fn retry_exchange_moves_exchange_behind_newer_one_and_completes_it_when_exchange_failed() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let (failed, old_call) = failed_exchange(&db, &user, &conversation).await;
    seed::message(&db, failed.id, MessageRole::Assistant, text("Par"), Some(old_call.id)).await;
    let newer = seed::exchange(&db, conversation.id, ExchangeStatus::Completed).await;
    seed::user_message(&db, newer.id, "Question B").await;
    let newer_call = seed::llm_call(&db, &user, newer.id).await;
    seed::message(&db, newer.id, MessageRole::Assistant, text("Answer B"), Some(newer_call.id)).await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 2, 12, 0, 0));
    let (llm, requests) = with_recorded_answers(vec![text_answer(&["Hello"], StopReason::EndTurn)]);
    let jobs = inline_jobs(&db, &registry, &clock, llm);

    let subscription = chat::retry_exchange(conversation.id, failed.id, &user, &db, &registry, &jobs, &*clock).await.expect("retrying the Exchange");

    let details = chat::details(conversation.id, &user, &db).await.expect("details");
    assert_eq!(details.exchanges.iter().map(|exchange| exchange.id).collect::<Vec<_>>(), vec![newer.id, failed.id]);
    assert_eq!(details.exchanges[1].status, ExchangeStatusView::Completed);
    assert_eq!(details.exchanges[1].blocks, vec![DisplayBlock::Text { text: "Hello".into() }]);
    assert_eq!(exchange(&db, failed.id).await.created_at, clock_time(&clock));
    let stored = messages_of(&db, failed.id).await;
    assert_eq!(stored.iter().map(|message| message.role.clone()).collect::<Vec<_>>(), vec![MessageRole::User, MessageRole::Assistant]);
    assert_eq!(stored[1].content, text("Hello"));
    let calls = LlmCalls::find().filter(llm_calls::Column::ExchangeId.eq(failed.id)).all(&*db).await.expect("querying LLM calls");
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().any(|call| call.id == old_call.id));
    let texts: Vec<String> = {
        let requests = requests.lock().expect("requests lock");
        requests[0].messages.iter().map(|message| message.content[0]["text"].as_str().unwrap_or("").to_owned()).collect()
    };
    assert_eq!(texts, vec!["Question B", "Answer B", "Question A"]);
    let events: Vec<_> = subscription.replay.into_iter().chain(drain(subscription.live.expect("live")).await).collect();
    assert_eq!(events.last(), Some(&ExchangeEvent::Completed { exchange_id: failed.id }));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn retry_exchange_deletes_only_trailing_assistant_message_when_partial_follows_tool_round() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let (failed, first_call) = failed_exchange(&db, &user, &conversation).await;
    let asked = seed::message(&db, failed.id, MessageRole::Assistant, tool_use("tu_1"), Some(first_call.id)).await;
    let answered = seed::message(&db, failed.id, MessageRole::Tool, json!([services::test_support::llm::date_time_result("tu_1")]), None).await;
    let second_call = seed::llm_call(&db, &user, failed.id).await;
    seed::message(&db, failed.id, MessageRole::Assistant, text("Par"), Some(second_call.id)).await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 2, 12, 0, 0));
    let (llm, requests) = with_recorded_answers(vec![text_answer(&["It is noon"], StopReason::EndTurn)]);
    let jobs = inline_jobs(&db, &registry, &clock, llm);

    chat::retry_exchange(conversation.id, failed.id, &user, &db, &registry, &jobs, &*clock).await.expect("retrying the Exchange");

    let sent: Vec<serde_json::Value> = {
        let requests = requests.lock().expect("requests lock");
        requests[0].messages.iter().map(|message| message.content[0]["type"].clone()).collect()
    };
    assert_eq!(sent, vec![json!("text"), json!("tool_use"), json!("tool_result")]);
    let stored = messages_of(&db, failed.id).await;
    assert_eq!(stored.len(), 4);
    assert_eq!((stored[1].id, stored[2].id), (asked.id, answered.id));
    assert_eq!(stored[3].content, text("It is noon"));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn retry_exchange_deletes_unanswered_tool_use_message_when_tool_round_failed() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let (failed, call) = failed_exchange(&db, &user, &conversation).await;
    let unanswered = seed::message(&db, failed.id, MessageRole::Assistant, tool_use("tu_1"), Some(call.id)).await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 2, 12, 0, 0));
    let (llm, requests) = with_recorded_answers(vec![text_answer(&["Hello"], StopReason::EndTurn)]);
    let jobs = inline_jobs(&db, &registry, &clock, llm);

    chat::retry_exchange(conversation.id, failed.id, &user, &db, &registry, &jobs, &*clock).await.expect("retrying the Exchange");

    assert_eq!(requests.lock().expect("requests lock")[0].messages.len(), 1);
    let stored = messages_of(&db, failed.id).await;
    assert_eq!(stored.iter().map(|message| message.role.clone()).collect::<Vec<_>>(), vec![MessageRole::User, MessageRole::Assistant]);
    assert!(stored.iter().all(|message| message.id != unanswered.id));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn retry_exchange_fails_with_exchange_in_progress_when_another_exchange_is_running() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let (failed, call) = failed_exchange(&db, &user, &conversation).await;
    seed::message(&db, failed.id, MessageRole::Assistant, text("Par"), Some(call.id)).await;
    seed::exchange(&db, conversation.id, ExchangeStatus::InProgress).await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 2, 12, 0, 0));
    let jobs = inline_jobs(&db, &registry, &clock, MockLlm::new());

    let result = chat::retry_exchange(conversation.id, failed.id, &user, &db, &registry, &jobs, &*clock).await;

    services::test_support::assert_single_violation(result, |violation| matches!(violation, RuleViolation::ConversationExchangeInProgress));
    let after = exchange(&db, failed.id).await;
    assert_eq!((after.status, after.created_at), (ExchangeStatus::Failed, seed::chat_time()));
    assert_eq!(messages_of(&db, failed.id).await.len(), 2);
    assert!(!registry.is_open(failed.id));
}

#[tokio::test]
#[ignore = "requires docker"]
async fn retry_exchange_rolls_back_status_and_message_delete_when_conversation_update_fails() {
    let db = Arc::new(database().await);
    let user = seed::user(&db, 1).await;
    let conversation = seed::conversation(&db, &user).await;
    let (failed, call) = failed_exchange(&db, &user, &conversation).await;
    seed::message(&db, failed.id, MessageRole::Assistant, text("Par"), Some(call.id)).await;
    seed::fail_on(&db, "conversations", "UPDATE").await;
    let registry = Arc::new(ExchangeRegistry::new());
    let clock = Arc::new(fixed_clock(2024, 1, 2, 12, 0, 0));
    let jobs = inline_jobs(&db, &registry, &clock, MockLlm::new());

    let result = chat::retry_exchange(conversation.id, failed.id, &user, &db, &registry, &jobs, &*clock).await;

    assert!(result.is_err());
    let after = exchange(&db, failed.id).await;
    assert_eq!((after.status, after.created_at), (ExchangeStatus::Failed, seed::chat_time()));
    assert_eq!(messages_of(&db, failed.id).await.len(), 2);
    assert!(!registry.is_open(failed.id));
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
