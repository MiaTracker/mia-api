//! A mock `Llm` and builders for the event streams and errors it returns.
//!
//! A `MockLlm` with no expectation for a method panics when that method is called, so a test
//! only sets up the calls the code under test should make.

use std::task::Poll;

use async_trait::async_trait;
use futures::StreamExt;
use integrations::openrouter::{BlockKind, Llm, LlmError, LlmEvent, LlmEventStream, LlmRequest, LlmUsage, StopReason};

mockall::mock! {
    pub Llm {}

    #[async_trait]
    impl Llm for Llm {
        fn model(&self) -> String;
        async fn stream_message(&self, request: LlmRequest) -> Result<LlmEventStream, LlmError>;
    }
}

/// The events of a complete text answer: request id `gen-test`, one text block holding `chunks`
/// as separate deltas, input usage 10 and output usage 5, ending with `stop`.
pub fn text_answer(chunks: &[&str], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>> {
    let mut events = vec![
        Ok(LlmEvent::MessageStart {
            request_id: Some("gen-test".into()),
            usage: LlmUsage { input_tokens: Some(10), ..LlmUsage::default() },
        }),
        Ok(LlmEvent::BlockStart { index: 0, kind: BlockKind::Text }),
    ];
    events.extend(chunks.iter().map(|chunk| Ok(LlmEvent::TextDelta { index: 0, text: chunk.to_string() })));
    events.push(Ok(LlmEvent::BlockStop { index: 0 }));
    events.push(Ok(LlmEvent::MessageDelta {
        stop_reason: Some(stop),
        usage: LlmUsage { output_tokens: Some(5), ..LlmUsage::default() },
    }));
    events.push(Ok(LlmEvent::MessageStop));
    events
}

/// The events of an answer holding only one thinking block with `thinking`, ending with `stop`.
pub fn thinking_answer(thinking: &str, stop: StopReason) -> Vec<Result<LlmEvent, LlmError>> {
    vec![
        Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() }),
        Ok(LlmEvent::BlockStart { index: 0, kind: BlockKind::Thinking }),
        Ok(LlmEvent::ThinkingDelta { index: 0, thinking: thinking.into() }),
        Ok(LlmEvent::BlockStop { index: 0 }),
        Ok(LlmEvent::MessageDelta { stop_reason: Some(stop), usage: LlmUsage::default() }),
        Ok(LlmEvent::MessageStop),
    ]
}

/// The events of an answer with one thinking block (index 0) and then one text block (index 1)
/// streamed in `chunks`, ending with `stop`.
pub fn thinking_then_text_answer(thinking: &str, chunks: &[&str], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>> {
    let mut events = vec![
        Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() }),
        Ok(LlmEvent::BlockStart { index: 0, kind: BlockKind::Thinking }),
        Ok(LlmEvent::ThinkingDelta { index: 0, thinking: thinking.into() }),
        Ok(LlmEvent::BlockStop { index: 0 }),
        Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::Text }),
    ];
    events.extend(chunks.iter().map(|chunk| Ok(LlmEvent::TextDelta { index: 1, text: chunk.to_string() })));
    events.push(Ok(LlmEvent::BlockStop { index: 1 }));
    events.push(Ok(LlmEvent::MessageDelta { stop_reason: Some(stop), usage: LlmUsage::default() }));
    events.push(Ok(LlmEvent::MessageStop));
    events
}

/// The events of an answer with one tool_use block per `(id, name, input)` in `calls`, at indexes
/// 0.., each input streamed as one `InputJsonDelta` (none for an empty object), ending with `stop`.
pub fn tool_use_answer(calls: &[(&str, &str, serde_json::Value)], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>> {
    let mut events = vec![Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() })];
    push_tool_uses(&mut events, 0, calls);
    events.push(Ok(LlmEvent::MessageDelta { stop_reason: Some(stop), usage: LlmUsage::default() }));
    events.push(Ok(LlmEvent::MessageStop));
    events
}

/// The events of an answer with one text block (index 0) streamed in `chunks` and then one tool_use
/// block per `(id, name, input)` in `calls`, ending with `stop`.
pub fn text_then_tool_use_answer(chunks: &[&str], calls: &[(&str, &str, serde_json::Value)], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>> {
    let mut events = vec![
        Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() }),
        Ok(LlmEvent::BlockStart { index: 0, kind: BlockKind::Text }),
    ];
    events.extend(chunks.iter().map(|chunk| Ok(LlmEvent::TextDelta { index: 0, text: chunk.to_string() })));
    events.push(Ok(LlmEvent::BlockStop { index: 0 }));
    push_tool_uses(&mut events, 1, calls);
    events.push(Ok(LlmEvent::MessageDelta { stop_reason: Some(stop), usage: LlmUsage::default() }));
    events.push(Ok(LlmEvent::MessageStop));
    events
}

/// The events of an answer with one thinking block (index 0) holding `thinking` and then one
/// tool_use block per `(id, name, input)` in `calls`, ending with `stop`.
pub fn thinking_then_tool_use_answer(thinking: &str, calls: &[(&str, &str, serde_json::Value)], stop: StopReason) -> Vec<Result<LlmEvent, LlmError>> {
    let mut events = vec![
        Ok(LlmEvent::MessageStart { request_id: Some("gen-test".into()), usage: LlmUsage::default() }),
        Ok(LlmEvent::BlockStart { index: 0, kind: BlockKind::Thinking }),
        Ok(LlmEvent::ThinkingDelta { index: 0, thinking: thinking.into() }),
        Ok(LlmEvent::BlockStop { index: 0 }),
    ];
    push_tool_uses(&mut events, 1, calls);
    events.push(Ok(LlmEvent::MessageDelta { stop_reason: Some(stop), usage: LlmUsage::default() }));
    events.push(Ok(LlmEvent::MessageStop));
    events
}

fn push_tool_uses(events: &mut Vec<Result<LlmEvent, LlmError>>, first_index: u32, calls: &[(&str, &str, serde_json::Value)]) {
    for (offset, (id, name, input)) in calls.iter().enumerate() {
        let index = first_index + offset as u32;
        events.push(Ok(LlmEvent::BlockStart { index, kind: BlockKind::ToolUse { id: id.to_string(), name: name.to_string() } }));
        // A tool without arguments streams no input, like the real client.
        if input.as_object().is_none_or(|object| !object.is_empty()) {
            events.push(Ok(LlmEvent::InputJsonDelta { index, partial_json: input.to_string() }));
        }
        events.push(Ok(LlmEvent::BlockStop { index }));
    }
}

/// A `date_time` call `id` for `tool_use_answer`, which takes no input.
pub fn date_time_call(id: &str) -> (&str, &'static str, serde_json::Value) {
    (id, "date_time", serde_json::json!({}))
}

/// The stored `tool_result` block answering `date_time` call `tool_use_id` at the time of
/// `fixed_clock(2024, 1, 1, 12, 0, 0)`.
pub fn date_time_result(tool_use_id: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "tool_result",
        "tool_use_id": tool_use_id,
        "content": r#"{"utc":"2024-01-01T12:00:00Z","timezone":"UTC"}"#,
        "mia_display": {"name": "date_time", "failed": false},
    })
}

/// A stream that yields `events` in order and then ends.
pub fn event_stream(events: Vec<Result<LlmEvent, LlmError>>) -> LlmEventStream {
    futures::stream::iter(events).boxed()
}

/// A stream that yields `events` in order and then ends, returning `Pending` once before each
/// event, like a network stream whose chunks arrive separately. Unlike [`event_stream`], no event
/// is ready right after another, so each one is written and published on its own.
pub fn trickled_stream(events: Vec<Result<LlmEvent, LlmError>>) -> LlmEventStream {
    futures::stream::iter(events)
        .then(|event| {
            let mut event = Some(event);
            let mut waited = false;
            futures::future::poll_fn(move |cx| {
                if !std::mem::replace(&mut waited, true) {
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                Poll::Ready(event.take().expect("polled after completion"))
            })
        })
        .boxed()
}

/// A stream that panics when polled, like a bug in the code reading the model's answer.
pub fn panicking_stream() -> LlmEventStream {
    futures::stream::poll_fn(|_| -> Poll<Option<Result<LlmEvent, LlmError>>> { panic!("stream panicked") }).boxed()
}

/// A stream that yields `events` in order and then never yields again nor ends.
pub fn stalled_stream(events: Vec<Result<LlmEvent, LlmError>>) -> LlmEventStream {
    futures::stream::iter(events).chain(futures::stream::pending()).boxed()
}

/// The error of a request the provider rejected with `status` before streaming.
pub fn http_error(status: u16, message: &str) -> LlmError {
    LlmError { status: Some(status), error_type: None, message: message.into(), response: None }
}

/// The error of a stream that failed midway with OpenRouter error type `error_type`.
pub fn stream_error(error_type: &str, message: &str) -> LlmError {
    LlmError {
        status: None,
        error_type: Some(error_type.into()),
        message: message.into(),
        response: Some(serde_json::json!({
            "type": "api_error",
            "message": message,
            "error_type": error_type,
        })),
    }
}

/// A `MockLlm` named `test/model` that expects exactly one `stream_message` and answers it
/// with `events`.
pub fn with_answer(events: Vec<Result<LlmEvent, LlmError>>) -> MockLlm {
    with_stream(event_stream(events))
}

/// A `MockLlm` named `test/model` that expects exactly one `stream_message` and answers it
/// with `stream`.
pub fn with_stream(stream: LlmEventStream) -> MockLlm {
    let mut llm = MockLlm::new();
    llm.expect_model().returning(|| "test/model".into());
    llm.expect_stream_message().times(1).return_once(move |_| Ok(stream));
    llm
}

/// A `MockLlm` named `test/model` that expects exactly `answers.len()` `stream_message` calls and
/// answers them in order, one element of `answers` per call.
pub fn with_answers(answers: Vec<Vec<Result<LlmEvent, LlmError>>>) -> MockLlm {
    let mut llm = MockLlm::new();
    llm.expect_model().returning(|| "test/model".into());
    let calls = answers.len();
    let answers = std::sync::Mutex::new(std::collections::VecDeque::from(answers));
    llm.expect_stream_message().times(calls).returning(move |_| {
        let events = answers.lock().expect("answers lock").pop_front().expect("more stream_message calls than answers");
        Ok(event_stream(events))
    });
    llm
}

/// Like `with_answers`, and also returns the requests the mock received, in call order.
pub fn with_recorded_answers(answers: Vec<Vec<Result<LlmEvent, LlmError>>>) -> (MockLlm, std::sync::Arc<std::sync::Mutex<Vec<LlmRequest>>>) {
    let mut llm = MockLlm::new();
    llm.expect_model().returning(|| "test/model".into());
    let calls = answers.len();
    let answers = std::sync::Mutex::new(std::collections::VecDeque::from(answers));
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorded = requests.clone();
    llm.expect_stream_message().times(calls).returning(move |request| {
        recorded.lock().expect("requests lock").push(request);
        let events = answers.lock().expect("answers lock").pop_front().expect("more stream_message calls than answers");
        Ok(event_stream(events))
    });
    (llm, requests)
}

/// A `MockLlm` named `test/model` that expects exactly one `stream_message` and rejects it
/// with `error` before streaming.
pub fn with_error(error: LlmError) -> MockLlm {
    let mut llm = MockLlm::new();
    llm.expect_model().returning(|| "test/model".into());
    llm.expect_stream_message().times(1).return_once(move |_| Err(error));
    llm
}

/// An `Llm` named `test/model` whose `stream_message` never resolves, like a provider that accepts
/// the connection but never answers. mockall cannot express a call that never returns.
pub struct UnansweringLlm;

#[async_trait]
impl Llm for UnansweringLlm {
    fn model(&self) -> String {
        "test/model".into()
    }

    async fn stream_message(&self, _request: LlmRequest) -> Result<LlmEventStream, LlmError> {
        std::future::pending().await
    }
}

#[cfg(test)]
mod tests {
    use futures::StreamExt;
    use integrations::openrouter::{LlmEvent, StopReason};
    use super::*;

    #[tokio::test]
    async fn text_answer_streams_text_between_block_start_and_stop() {
        let events: Vec<_> = event_stream(text_answer(&["Hel", "lo"], StopReason::EndTurn)).collect().await;

        assert_eq!(events[2], Ok(LlmEvent::TextDelta { index: 0, text: "Hel".into() }));
        assert_eq!(events[3], Ok(LlmEvent::TextDelta { index: 0, text: "lo".into() }));
        assert_eq!(events.last(), Some(&Ok(LlmEvent::MessageStop)));
    }

    #[tokio::test]
    async fn with_answers_answers_calls_in_order() {
        let llm = with_answers(vec![text_answer(&["one"], StopReason::EndTurn), text_answer(&["two"], StopReason::EndTurn)]);
        let request = || LlmRequest { system: String::new(), messages: Vec::new(), tools: Vec::new() };

        let first: Vec<_> = llm.stream_message(request()).await.expect("first").collect().await;
        let second: Vec<_> = llm.stream_message(request()).await.expect("second").collect().await;

        assert_eq!(first[2], Ok(LlmEvent::TextDelta { index: 0, text: "one".into() }));
        assert_eq!(second[2], Ok(LlmEvent::TextDelta { index: 0, text: "two".into() }));
    }
}
