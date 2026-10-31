//! Turns the stream events of one LLM call into stored Anthropic content blocks.
//!
//! The accumulator is a pure state machine. The caller writes `content()` to the database and
//! only then forwards the deltas `apply` returned, so stream and database always agree.

use integrations::openrouter::{BlockKind, LlmEvent, LlmUsage, StopReason};
use serde_json::{json, Value};
use views::conversations::ExchangeEvent;

enum Body {
    Text {
        text: String,
        /// The Exchange-wide number, set when the first non-empty delta arrives.
        block: Option<u32>,
    },
    Thinking {
        thinking: String,
        signature: String,
        /// The Exchange-wide number, set when the first non-empty thinking delta arrives.
        block: Option<u32>,
    },
    ToolUse {
        id: String,
        name: String,
        /// The concatenated `InputJsonDelta`s.
        json: String,
        /// Whether `BlockStop` arrived.
        stopped: bool,
    },
    RedactedThinking {
        data: String,
    },
}

impl Body {
    /// The input of a tool_use body when its JSON parses to an object. Empty JSON is the empty
    /// object: the provider streams no input for a tool without arguments.
    fn tool_input(json: &str) -> Option<Value> {
        if json.trim().is_empty() {
            return Some(json!({}));
        }
        serde_json::from_str::<Value>(json).ok().filter(Value::is_object)
    }
}

/// One tool_use block the model produced.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    /// `None` when the streamed JSON does not parse to an object.
    pub input: Option<Value>,
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
    /// `first_block` is the Exchange-wide number of the first displayed block this call produces.
    pub fn new(first_block: u32) -> Self {
        Self { next_block: first_block, blocks: Vec::new(), stop_reason: None, message_stopped: false, usage: LlmUsage::default(), request_id: None }
    }

    /// Applies one event and returns the `TextDelta` and `ThinkingDelta` events to forward to the
    /// client.
    pub fn apply(&mut self, event: LlmEvent) -> Vec<ExchangeEvent> {
        match event {
            LlmEvent::MessageStart { request_id, usage } => {
                self.request_id = request_id.or(self.request_id.take());
                self.merge_usage(usage);
            }
            LlmEvent::BlockStart { index, kind } => {
                let body = match kind {
                    BlockKind::Text => Body::Text { text: String::new(), block: None },
                    BlockKind::Thinking => Body::Thinking { thinking: String::new(), signature: String::new(), block: None },
                    BlockKind::ToolUse { id, name } => Body::ToolUse { id, name, json: String::new(), stopped: false },
                    BlockKind::RedactedThinking { data } => Body::RedactedThinking { data },
                    BlockKind::Other(_) => return Vec::new(),
                };
                self.blocks.push(Block { index, body });
            }
            LlmEvent::TextDelta { index, text } => {
                if text.is_empty() {
                    return Vec::new();
                }
                if let Some(Block { body: Body::Text { text: stored, block }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    let block = number(block, &mut self.next_block);
                    stored.push_str(&text);
                    return vec![ExchangeEvent::TextDelta { block, text }];
                }
            }
            LlmEvent::ThinkingDelta { index, thinking } => {
                if thinking.is_empty() {
                    return Vec::new();
                }
                if let Some(Block { body: Body::Thinking { thinking: stored, block, .. }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    let block = number(block, &mut self.next_block);
                    stored.push_str(&thinking);
                    return vec![ExchangeEvent::ThinkingDelta { block, text: thinking }];
                }
            }
            LlmEvent::SignatureDelta { index, signature } => {
                if let Some(Block { body: Body::Thinking { signature: stored, .. }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    stored.push_str(&signature);
                }
            }
            LlmEvent::InputJsonDelta { index, partial_json } => {
                if let Some(Block { body: Body::ToolUse { json, .. }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    json.push_str(&partial_json);
                }
            }
            LlmEvent::BlockStop { index } => {
                if let Some(Block { body: Body::ToolUse { stopped, .. }, .. }) = self.blocks.iter_mut().find(|block| block.index == index) {
                    *stopped = true;
                }
            }
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

    /// The Anthropic content block array; text and thinking blocks without text are left out. A
    /// tool_use block whose input is not (yet) a JSON object is stored with the input `{}`.
    pub fn content(&self) -> Value {
        Value::Array(
            self.blocks
                .iter()
                .filter_map(|block| match &block.body {
                    Body::Text { text, .. } if !text.is_empty() => Some(json!({"type": "text", "text": text})),
                    Body::Thinking { thinking, signature, .. } if !thinking.is_empty() => {
                        Some(json!({"type": "thinking", "thinking": thinking, "signature": signature}))
                    }
                    Body::ToolUse { id, name, json, .. } => {
                        Some(json!({"type": "tool_use", "id": id, "name": name, "input": Body::tool_input(json).unwrap_or_else(|| json!({}))}))
                    }
                    Body::RedactedThinking { data } => Some(json!({"type": "redacted_thinking", "data": data})),
                    _ => None,
                })
                .collect(),
        )
    }

    /// Whether any text block holds text, or any thinking, tool_use or redacted_thinking block exists.
    pub fn has_content(&self) -> bool {
        self.blocks.iter().any(|block| match &block.body {
            Body::Text { text, .. } => !text.is_empty(),
            Body::Thinking { thinking, .. } => !thinking.is_empty(),
            Body::ToolUse { .. } | Body::RedactedThinking { .. } => true,
        })
    }

    /// Every tool_use block, in the order the provider sent them.
    pub fn tool_uses(&self) -> Vec<ToolUse> {
        self.blocks
            .iter()
            .filter_map(|block| match &block.body {
                Body::ToolUse { id, name, json, .. } => Some(ToolUse { id: id.clone(), name: name.clone(), input: Body::tool_input(json) }),
                _ => None,
            })
            .collect()
    }

    /// Whether the model hit the token limit while writing a tool call: the stop reason is
    /// `MaxTokens` and some tool_use block never stopped or holds no JSON object.
    pub fn tool_use_cut(&self) -> bool {
        self.stop_reason == Some(StopReason::MaxTokens)
            && self.blocks.iter().any(|block| matches!(&block.body, Body::ToolUse { json, stopped, .. } if !stopped || Body::tool_input(json).is_none()))
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

/// The Exchange-wide number of a block, taking the next one from `next_block` the first time.
fn number(block: &mut Option<u32>, next_block: &mut u32) -> u32 {
    *block.get_or_insert_with(|| {
        let number = *next_block;
        *next_block += 1;
        number
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use views::conversations::ExchangeEvent;
    use super::*;
    use crate::test_support::init;
    use crate::test_support::llm::{text_answer, text_then_tool_use_answer, tool_use_answer};

    fn start(index: u32, kind: BlockKind) -> LlmEvent {
        LlmEvent::BlockStart { index, kind }
    }

    fn text(index: u32, text: &str) -> LlmEvent {
        LlmEvent::TextDelta { index, text: text.into() }
    }

    fn delta(block: u32, text: &str) -> ExchangeEvent {
        ExchangeEvent::TextDelta { block, text: text.into() }
    }

    fn thinking_delta(block: u32, text: &str) -> ExchangeEvent {
        ExchangeEvent::ThinkingDelta { block, text: text.into() }
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

        assert_eq!(deltas, vec![delta(2, "Hi")]);
    }

    #[test]
    fn apply_numbers_text_after_thinking_when_thinking_came_first() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Thinking));
        accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "hmm".into() });
        accumulator.apply(start(1, BlockKind::Text));

        let deltas = accumulator.apply(text(1, "Answer"));

        assert_eq!(deltas, vec![delta(1, "Answer")]);
    }

    #[test]
    fn apply_returns_thinking_delta_with_block_when_thinking_delta() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Thinking));

        let deltas = accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "hmm".into() });

        assert_eq!(deltas, vec![thinking_delta(0, "hmm")]);
    }

    #[test]
    fn apply_returns_nothing_when_thinking_block_gets_only_signature_or_empty_deltas() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Thinking));

        assert_eq!(accumulator.apply(LlmEvent::ThinkingDelta { index: 0, thinking: "".into() }), vec![]);
        assert_eq!(accumulator.apply(LlmEvent::SignatureDelta { index: 0, signature: "s".into() }), vec![]);
        accumulator.apply(start(1, BlockKind::Text));
        assert_eq!(accumulator.apply(text(1, "Hi")), vec![delta(0, "Hi")]);
    }

    #[test]
    fn apply_numbers_blocks_by_first_non_empty_delta_when_empty_block_precedes() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::Text));
        accumulator.apply(start(1, BlockKind::Text));

        assert_eq!(accumulator.apply(text(0, "")), vec![]);
        assert_eq!(accumulator.apply(text(1, "B")), vec![delta(0, "B")]);
        assert_eq!(accumulator.apply(text(0, "A")), vec![delta(1, "A")]);
        assert_eq!(accumulator.apply(text(1, "b")), vec![delta(0, "b")]);
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

        assert_eq!(deltas, vec![delta(0, "Hel"), delta(0, "lo")]);
        assert_eq!(accumulator.content(), json!([{"type": "text", "text": "Hello"}]));
        assert!(accumulator.has_text());
        assert!(accumulator.finished());
        assert_eq!(accumulator.request_id(), Some("gen-test"));
    }

    fn feed(accumulator: &mut Accumulator, events: Vec<Result<LlmEvent, integrations::openrouter::LlmError>>) -> Vec<ExchangeEvent> {
        events.into_iter().flat_map(|event| accumulator.apply(event.expect("event"))).collect()
    }

    #[test]
    fn content_includes_tool_use_with_parsed_input_when_block_stops() {
        init();
        let mut accumulator = Accumulator::new(0);
        feed(&mut accumulator, tool_use_answer(&[("tu_1", "mia_search", json!({"query": "Alien"}))], StopReason::ToolUse));

        assert_eq!(accumulator.content(), json!([{"type": "tool_use", "id": "tu_1", "name": "mia_search", "input": {"query": "Alien"}}]));
        let tool_uses = accumulator.tool_uses();
        assert_eq!(tool_uses.len(), 1);
        assert_eq!(tool_uses[0].id, "tu_1");
        assert_eq!(tool_uses[0].name, "mia_search");
        assert_eq!(tool_uses[0].input, Some(json!({"query": "Alien"})));
    }

    #[test]
    fn content_includes_tool_use_with_empty_input_when_input_is_not_an_object() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::ToolUse { id: "tu_1".into(), name: "mia_search".into() }));
        accumulator.apply(LlmEvent::InputJsonDelta { index: 0, partial_json: "[1,2".into() });
        accumulator.apply(LlmEvent::BlockStop { index: 0 });

        assert_eq!(accumulator.content(), json!([{"type": "tool_use", "id": "tu_1", "name": "mia_search", "input": {}}]));
        assert_eq!(accumulator.tool_uses()[0].input, None);
    }

    #[test]
    fn tool_uses_returns_every_call_in_order_when_several_tool_use_blocks_arrive() {
        init();
        let mut accumulator = Accumulator::new(0);
        feed(
            &mut accumulator,
            text_then_tool_use_answer(
                &["Let me look."],
                &[("tu_1", "mia_search", json!({"query": "a"})), ("tu_2", "date_time", json!({}))],
                StopReason::ToolUse,
            ),
        );

        let ids: Vec<_> = accumulator.tool_uses().into_iter().map(|tool_use| tool_use.id).collect();
        assert_eq!(ids, vec!["tu_1", "tu_2"]);
        assert_eq!(
            accumulator.content(),
            json!([
                {"type": "text", "text": "Let me look."},
                {"type": "tool_use", "id": "tu_1", "name": "mia_search", "input": {"query": "a"}},
                {"type": "tool_use", "id": "tu_2", "name": "date_time", "input": {}},
            ])
        );
    }

    #[test]
    fn apply_returns_no_deltas_and_numbers_nothing_when_tool_use_streams() {
        init();
        let mut accumulator = Accumulator::new(3);
        let deltas = feed(&mut accumulator, tool_use_answer(&[("tu_1", "mia_search", json!({"query": "x"}))], StopReason::ToolUse));
        assert_eq!(deltas, vec![]);

        accumulator.apply(start(1, BlockKind::Text));

        assert_eq!(accumulator.apply(text(1, "Hi")), vec![delta(3, "Hi")]);
    }

    #[test]
    fn has_content_is_true_and_has_text_false_when_only_tool_use_arrived() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::ToolUse { id: "tu_1".into(), name: "date_time".into() }));

        assert!(accumulator.has_content());
        assert!(!accumulator.has_text());
    }

    #[test]
    fn tool_use_cut_is_true_when_max_tokens_stops_unfinished_tool_use() {
        init();
        let mut cut = Accumulator::new(0);
        cut.apply(start(0, BlockKind::ToolUse { id: "tu_1".into(), name: "mia_search".into() }));
        cut.apply(LlmEvent::InputJsonDelta { index: 0, partial_json: "{\"query\": \"Al".into() });
        cut.apply(LlmEvent::MessageDelta { stop_reason: Some(StopReason::MaxTokens), usage: LlmUsage::default() });
        assert!(cut.tool_use_cut());

        let mut complete = Accumulator::new(0);
        feed(&mut complete, tool_use_answer(&[("tu_1", "mia_search", json!({"query": "Alien"}))], StopReason::MaxTokens));
        assert!(!complete.tool_use_cut());

        let mut invalid = Accumulator::new(0);
        invalid.apply(start(0, BlockKind::ToolUse { id: "tu_1".into(), name: "mia_search".into() }));
        invalid.apply(LlmEvent::InputJsonDelta { index: 0, partial_json: "[1,2".into() });
        invalid.apply(LlmEvent::BlockStop { index: 0 });
        invalid.apply(LlmEvent::MessageDelta { stop_reason: Some(StopReason::EndTurn), usage: LlmUsage::default() });
        assert!(!invalid.tool_use_cut());
    }

    #[test]
    fn tool_use_cut_is_true_when_max_tokens_stops_tool_use_with_unparseable_input() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::ToolUse { id: "tu_1".into(), name: "mia_search".into() }));
        accumulator.apply(LlmEvent::InputJsonDelta { index: 0, partial_json: "[1,2".into() });
        accumulator.apply(LlmEvent::BlockStop { index: 0 });
        accumulator.apply(LlmEvent::MessageDelta { stop_reason: Some(StopReason::MaxTokens), usage: LlmUsage::default() });

        assert!(accumulator.tool_use_cut());
    }

    #[test]
    fn tool_uses_returns_empty_object_input_when_no_input_delta_arrived() {
        init();
        let mut accumulator = Accumulator::new(0);
        accumulator.apply(start(0, BlockKind::ToolUse { id: "tu_1".into(), name: "date_time".into() }));
        accumulator.apply(LlmEvent::BlockStop { index: 0 });
        accumulator.apply(LlmEvent::MessageDelta { stop_reason: Some(StopReason::MaxTokens), usage: LlmUsage::default() });

        assert_eq!(accumulator.tool_uses()[0].input, Some(json!({})));
        assert_eq!(accumulator.content(), json!([{"type": "tool_use", "id": "tu_1", "name": "date_time", "input": {}}]));
        assert!(!accumulator.tool_use_cut());
    }

    #[test]
    fn content_includes_redacted_thinking_when_block_starts() {
        init();
        let mut accumulator = Accumulator::new(0);
        assert_eq!(accumulator.apply(start(0, BlockKind::RedactedThinking { data: "x".into() })), vec![]);

        assert_eq!(accumulator.content(), json!([{"type": "redacted_thinking", "data": "x"}]));
        assert!(accumulator.has_content());
        assert!(!accumulator.has_text());
    }
}
