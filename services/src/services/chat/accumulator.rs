//! Turns the stream events of one LLM call into stored Anthropic content blocks.
//!
//! The accumulator is a pure state machine. The caller writes `content()` to the database and
//! only then forwards the deltas `apply` returned, so stream and database always agree.

use integrations::openrouter::{BlockKind, LlmEvent, LlmUsage, StopReason};
use serde_json::{json, Value};

enum Body {
    Text {
        text: String,
        /// The Exchange-wide number, set when the first non-empty delta arrives.
        block: Option<u32>,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
}

struct Block {
    /// The provider's index of this block within the call.
    index: u32,
    body: Body,
}

pub struct Accumulator {
    next_block: u32,
    blocks: Vec<Block>,
    stop_reason: Option<StopReason>,
    /// Whether `MessageStop` arrived.
    message_stopped: bool,
    usage: LlmUsage,
    request_id: Option<String>,
}

impl Accumulator {
    /// `first_block` is the Exchange-wide number of the first text block this call produces.
    pub fn new(first_block: u32) -> Self {
        Self { next_block: first_block, blocks: Vec::new(), stop_reason: None, message_stopped: false, usage: LlmUsage::default(), request_id: None }
    }

    /// Applies one event and returns the display deltas `(block, text)` to forward to the client.
    pub fn apply(&mut self, event: LlmEvent) -> Vec<(u32, String)> {
        match event {
            LlmEvent::MessageStart { request_id, usage } => {
                self.request_id = request_id.or(self.request_id.take());
                self.merge_usage(usage);
            }
            LlmEvent::BlockStart { index, kind } => {
                let body = match kind {
                    BlockKind::Text => Body::Text { text: String::new(), block: None },
                    BlockKind::Thinking => Body::Thinking { thinking: String::new(), signature: String::new() },
                    BlockKind::Other(_) => return Vec::new(),
                };
                self.blocks.push(Block { index, body });
            }
            LlmEvent::TextDelta { index, text } => {
                if text.is_empty() {
                    return Vec::new();
                }
                if let Some(Block { body: Body::Text { text: stored, block }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    let number = *block.get_or_insert_with(|| {
                        let number = self.next_block;
                        self.next_block += 1;
                        number
                    });
                    stored.push_str(&text);
                    return vec![(number, text)];
                }
            }
            LlmEvent::ThinkingDelta { index, thinking } => {
                if let Some(Block { body: Body::Thinking { thinking: stored, .. }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    stored.push_str(&thinking);
                }
            }
            LlmEvent::SignatureDelta { index, signature } => {
                if let Some(Block { body: Body::Thinking { signature: stored, .. }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    stored.push_str(&signature);
                }
            }
            LlmEvent::BlockStop { .. } => {}
            LlmEvent::MessageStop => self.message_stopped = true,
            LlmEvent::MessageDelta { stop_reason, usage } => {
                self.stop_reason = stop_reason.or(self.stop_reason.take());
                self.merge_usage(usage);
            }
        }
        Vec::new()
    }

    fn merge_usage(&mut self, usage: LlmUsage) {
        self.usage = LlmUsage {
            input_tokens: usage.input_tokens.or(self.usage.input_tokens),
            output_tokens: usage.output_tokens.or(self.usage.output_tokens),
            cache_read_tokens: usage.cache_read_tokens.or(self.usage.cache_read_tokens),
            cache_creation_tokens: usage.cache_creation_tokens.or(self.usage.cache_creation_tokens),
        };
    }

    /// The Anthropic content block array; blocks without text are left out.
    pub fn content(&self) -> Value {
        Value::Array(
            self.blocks
                .iter()
                .filter_map(|block| match &block.body {
                    Body::Text { text, .. } if !text.is_empty() => Some(json!({"type": "text", "text": text})),
                    Body::Thinking { thinking, signature } if !thinking.is_empty() => {
                        Some(json!({"type": "thinking", "thinking": thinking, "signature": signature}))
                    }
                    _ => None,
                })
                .collect(),
        )
    }

    /// Whether any text or thinking block holds text.
    pub fn has_content(&self) -> bool {
        self.blocks.iter().any(|block| match &block.body {
            Body::Text { text, .. } => !text.is_empty(),
            Body::Thinking { thinking, .. } => !thinking.is_empty(),
        })
    }

    /// Whether any text block holds text.
    pub fn has_text(&self) -> bool {
        self.blocks.iter().any(|block| matches!(&block.body, Body::Text { text, .. } if !text.is_empty()))
    }

    pub fn stop_reason(&self) -> Option<&StopReason> {
        self.stop_reason.as_ref()
    }

    /// Whether the model finished its message: a stop reason or `MessageStop` arrived.
    pub fn finished(&self) -> bool {
        self.stop_reason.is_some() || self.message_stopped
    }

    /// Usage from `MessageStart` and `MessageDelta` merged; a later value replaces an earlier one
    /// and an absent value never clears one.
    pub fn usage(&self) -> &LlmUsage {
        &self.usage
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use super::*;
    use crate::test_support::init;
    use crate::test_support::llm::text_answer;

    fn start(index: u32, kind: BlockKind) -> LlmEvent {
        LlmEvent::BlockStart { index, kind }
    }

    fn text(index: u32, text: &str) -> LlmEvent {
        LlmEvent::TextDelta { index, text: text.into() }
    }

    fn message_start(usage: LlmUsage) -> LlmEvent {
        LlmEvent::MessageStart { request_id: Some("gen-1".into()), usage }
    }

    #[test]
    fn apply_returns_text_with_exchange_wide_block_when_text_delta() {
        init();
        let mut accumulator = Accumulator::new(2);
        accumulator.apply(start(0, BlockKind::Text));

        let deltas = accumulator.apply(text(0, "Hi"));

        assert_eq!(deltas, vec![(2, "Hi".to_string())]);
    }

    #[test]
    fn apply_numbers_second_text_block_next_when_provider_index_skips_thinking() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Thinking));
        accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "hmm".into() });
        accumulator.apply(start(1, BlockKind::Text));

        let deltas = accumulator.apply(text(1, "Answer"));

        assert_eq!(deltas, vec![(0, "Answer".to_string())]);
    }

    #[test]
    fn apply_numbers_blocks_by_first_non_empty_delta_when_empty_block_precedes() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Text));
        accumulator.apply(start(1, BlockKind::Text));

        assert_eq!(accumulator.apply(text(0, "")), vec![]);
        assert_eq!(accumulator.apply(text(1, "B")), vec![(0, "B".to_string())]);
        assert_eq!(accumulator.apply(text(0, "A")), vec![(1, "A".to_string())]);
        assert_eq!(accumulator.apply(text(1, "b")), vec![(0, "b".to_string())]);
    }

    #[test]
    fn content_omits_empty_text_block_when_no_delta_arrived() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Text));
        accumulator.apply(start(1, BlockKind::Text));
        accumulator.apply(text(1, "Hi"));

        assert_eq!(accumulator.content(), json!([{"type": "text", "text": "Hi"}]));
    }

    #[test]
    fn content_holds_thinking_with_signature_when_thinking_streamed() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Thinking));
        accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "let me ".into() });
        accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "think".into() });
        accumulator.apply(LlmEvent::SignatureDelta { index: 0, signature: "sig".into() });

        assert_eq!(accumulator.content(), json!([{"type": "thinking", "thinking": "let me think", "signature": "sig"}]));
        assert!(accumulator.has_content());
        assert!(!accumulator.has_text());
    }

    #[test]
    fn content_stores_empty_signature_when_none_arrived() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Thinking));
        accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "x".into() });

        assert_eq!(accumulator.content(), json!([{"type": "thinking", "thinking": "x", "signature": ""}]));
    }

    #[test]
    fn has_content_returns_false_when_only_message_start_arrived() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(message_start(LlmUsage::default()));
        accumulator.apply(start(0, BlockKind::Text));

        assert!(!accumulator.has_content());
        assert!(!accumulator.has_text());
        assert_eq!(accumulator.content(), json!([]));
    }

    #[test]
    fn apply_ignores_other_blocks_when_provider_sends_unknown_kind() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Other("tool_use".into())));

        assert_eq!(accumulator.apply(text(0, "x")), vec![]);
        assert!(!accumulator.has_content());
    }

    #[test]
    fn usage_merges_input_from_start_and_output_from_delta() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(message_start(LlmUsage { input_tokens: Some(10), cache_read_tokens: Some(3), ..LlmUsage::default() }));
        accumulator.apply(LlmEvent::MessageDelta {
            stop_reason: Some(StopReason::EndTurn),
            usage: LlmUsage { output_tokens: Some(5), ..LlmUsage::default() },
        });

        assert_eq!(
            accumulator.usage(),
            &LlmUsage { input_tokens: Some(10), output_tokens: Some(5), cache_read_tokens: Some(3), cache_creation_tokens: None }
        );
        assert_eq!(accumulator.stop_reason(), Some(&StopReason::EndTurn));
        assert_eq!(accumulator.request_id(), Some("gen-1"));
    }

    #[test]
    fn usage_keeps_earlier_value_when_later_event_reports_none() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(message_start(LlmUsage { input_tokens: Some(10), ..LlmUsage::default() }));
        accumulator.apply(LlmEvent::MessageDelta {
            stop_reason: None,
            usage: LlmUsage { input_tokens: Some(12), output_tokens: Some(5), ..LlmUsage::default() },
        });
        accumulator.apply(LlmEvent::MessageDelta { stop_reason: None, usage: LlmUsage::default() });

        assert_eq!(accumulator.usage().input_tokens, Some(12));
        assert_eq!(accumulator.usage().output_tokens, Some(5));
        assert_eq!(accumulator.stop_reason(), None);
    }

    #[test]
    fn finished_returns_false_when_text_arrived_without_stop() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Text));
        accumulator.apply(text(0, "Hel"));

        assert!(!accumulator.finished());
    }

    #[test]
    fn finished_returns_true_when_message_stop_arrived_without_stop_reason() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(LlmEvent::MessageStop);

        assert!(accumulator.finished());
        assert_eq!(accumulator.stop_reason(), None);
    }

    #[test]
    fn accumulate_reproduces_text_when_fed_full_text_answer() {
        init();
        let mut accumulator = Accumulator::new(0);
        let mut deltas = Vec::new();
        for event in text_answer(&["Hel", "lo"], StopReason::EndTurn) {
            deltas.extend(accumulator.apply(event.expect("event")));
        }

        assert_eq!(deltas, vec![(0, "Hel".to_string()), (0, "lo".to_string())]);
        assert_eq!(accumulator.content(), json!([{"type": "text", "text": "Hello"}]));
        assert!(accumulator.has_text());
        assert!(accumulator.finished());
        assert_eq!(accumulator.request_id(), Some("gen-test"));
    }
}
