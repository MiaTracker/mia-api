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
}
