//! The display rule: which stored content blocks a client sees.

use entities::messages;
use entities::sea_orm_active_enums::MessageRole;
use serde_json::Value;
use views::conversations::{DisplayBlock, ToolCallView};

/// The field of a stored `tool_result` block that holds its display form. It is stripped from the
/// block before the block is sent to the model.
pub const DISPLAY_FIELD: &str = "mia_display";

/// The non-empty text and thinking blocks of assistant Messages and the tool call blocks of tool
/// Messages, in message order and then block order. A tool Message follows the assistant Message
/// whose calls it answers, so its blocks come after that Message's own. Tool blocks come from the
/// `mia_display` of the tool Message's `tool_result` blocks in stored order, which is the order of
/// the `tool_use` blocks they answer, since the results are written in that order; no matching by
/// id is needed. This is the numbering the stream uses for the `block` of `text_delta`,
/// `thinking_delta` and `tool_call`.
pub fn display_blocks(messages: &[messages::Model]) -> Vec<DisplayBlock> {
    messages
        .iter()
        .flat_map(|message| {
            let blocks = message.content.as_array().into_iter().flatten();
            blocks.filter_map(move |block| match message.role {
                MessageRole::Assistant => display_block(block),
                MessageRole::Tool => tool_block(block),
                MessageRole::User => None,
            })
        })
        .collect()
}

/// The tool call shown for one `tool_result` block; `None` for other kinds and for results
/// without a readable `mia_display`.
fn tool_block(block: &Value) -> Option<DisplayBlock> {
    if block["type"] != "tool_result" {
        return None;
    }
    let call = serde_json::from_value::<ToolCallView>(block[DISPLAY_FIELD].clone()).ok()?;
    Some(DisplayBlock::Tool(call))
}

/// The displayed form of one stored content block; `None` for other kinds and for empty blocks.
fn display_block(block: &Value) -> Option<DisplayBlock> {
    let (field, shown): (&str, fn(String) -> DisplayBlock) = match block["type"].as_str()? {
        "text" => ("text", |text| DisplayBlock::Text { text }),
        "thinking" => ("thinking", |thinking| DisplayBlock::Thinking { thinking }),
        _ => return None,
    };
    let text = block[field].as_str().filter(|text| !text.is_empty())?;
    Some(shown(text.to_string()))
}

/// The text of a user message: all its text blocks concatenated.
pub fn user_text(message: &messages::Model) -> String {
    text_blocks(message).collect()
}

/// The `text` of each text block of `message`, in order.
fn text_blocks(message: &messages::Model) -> impl Iterator<Item = &str> {
    message
        .content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
}

#[cfg(test)]
mod tests {
    use integrations::openrouter::{BlockKind, LlmEvent};
    use serde_json::json;
    use super::*;
    use crate::services::chat::accumulator::Accumulator;
    use crate::test_support::fixtures::message_model;
    use crate::test_support::init;
    use views::conversations::ExchangeEvent;

    fn text_event(index: u32, text: &str) -> LlmEvent {
        LlmEvent::TextDelta { index, text: text.into() }
    }

    #[test]
    fn display_blocks_matches_stream_numbering_when_content_reloaded() {
        init();
        let events = vec![
            LlmEvent::BlockStart { index: 0, kind: BlockKind::Thinking },
            LlmEvent::ThinkingDelta { index: 0, thinking: "hmm".into() },
            LlmEvent::BlockStop { index: 0 },
            LlmEvent::BlockStart { index: 1, kind: BlockKind::Text },
            text_event(1, "Hel"),
            text_event(1, "lo"),
            LlmEvent::BlockStop { index: 1 },
            LlmEvent::BlockStart { index: 2, kind: BlockKind::Text },
            LlmEvent::BlockStop { index: 2 },
            LlmEvent::BlockStart { index: 3, kind: BlockKind::Text },
            text_event(3, "World"),
            LlmEvent::BlockStop { index: 3 },
        ];
        let mut accumulator = Accumulator::new(0);
        let mut streamed = std::collections::BTreeMap::<u32, DisplayBlock>::new();
        for event in events {
            for delta in accumulator.apply(event) {
                match delta {
                    ExchangeEvent::TextDelta { block, text } => match streamed.entry(block).or_insert(DisplayBlock::Text { text: String::new() }) {
                        DisplayBlock::Text { text: stored } => stored.push_str(&text),
                        other => panic!("text delta for {other:?}"),
                    },
                    ExchangeEvent::ThinkingDelta { block, text } => match streamed.entry(block).or_insert(DisplayBlock::Thinking { thinking: String::new() }) {
                        DisplayBlock::Thinking { thinking } => thinking.push_str(&text),
                        other => panic!("thinking delta for {other:?}"),
                    },
                    other => panic!("unexpected event {other:?}"),
                }
            }
        }
        let message = message_model(1, 1, MessageRole::Assistant, accumulator.content());

        let blocks = display_blocks(&[message]);

        let expected: Vec<DisplayBlock> = streamed.into_values().collect();
        assert_eq!(blocks, expected);
        assert_eq!(blocks, vec![
            DisplayBlock::Thinking { thinking: "hmm".into() },
            DisplayBlock::Text { text: "Hello".into() },
            DisplayBlock::Text { text: "World".into() },
        ]);
    }

    #[test]
    fn display_blocks_includes_thinking_and_skips_empty_blocks_when_stored() {
        init();
        let user = message_model(1, 1, MessageRole::User, json!([{"type": "text", "text": "question"}]));
        let tool = message_model(2, 1, MessageRole::Tool, json!([{"type": "text", "text": "tool output"}]));
        let assistant = message_model(
            3,
            1,
            MessageRole::Assistant,
            json!([
                {"type": "thinking", "thinking": "hmm", "signature": ""},
                {"type": "thinking", "thinking": "", "signature": "s"},
                {"type": "text", "text": ""},
                {"type": "text", "text": "answer"}
            ]),
        );

        let blocks = display_blocks(&[user, tool, assistant]);

        assert_eq!(blocks, vec![DisplayBlock::Thinking { thinking: "hmm".into() }, DisplayBlock::Text { text: "answer".into() }]);
    }

    fn search_view() -> ToolCallView {
        ToolCallView { name: "mia_search".into(), query: Some("Alien".into()), media_type: None, count: Some(2), titles: None, failed: false }
    }

    fn date_time_view() -> ToolCallView {
        ToolCallView { name: "date_time".into(), query: None, media_type: None, count: None, titles: None, failed: false }
    }

    #[test]
    fn display_blocks_places_tool_blocks_after_their_message_blocks() {
        init();
        let user = message_model(1, 1, MessageRole::User, json!([{"type": "text", "text": "question"}]));
        let calling = message_model(2, 1, MessageRole::Assistant, json!([
            {"type": "thinking", "thinking": "hm", "signature": ""},
            {"type": "tool_use", "id": "tu_1", "name": "mia_search", "input": {}},
            {"type": "tool_use", "id": "tu_2", "name": "date_time", "input": {}}
        ]));
        let results = message_model(3, 1, MessageRole::Tool, json!([
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "found", "mia_display": {"name": "mia_search", "query": "Alien", "count": 2, "failed": false}},
            {"type": "tool_result", "tool_use_id": "tu_2", "content": "now", "mia_display": {"name": "date_time", "failed": false}}
        ]));
        let answer = message_model(4, 1, MessageRole::Assistant, json!([{"type": "text", "text": "Done"}]));

        let blocks = display_blocks(&[user, calling, results, answer]);

        assert_eq!(blocks, vec![
            DisplayBlock::Thinking { thinking: "hm".into() },
            DisplayBlock::Tool(search_view()),
            DisplayBlock::Tool(date_time_view()),
            DisplayBlock::Text { text: "Done".into() },
        ]);
    }

    #[test]
    fn display_blocks_skips_tool_use_when_no_tool_result_exists() {
        init();
        let assistant = message_model(1, 1, MessageRole::Assistant, json!([
            {"type": "text", "text": "Hi"},
            {"type": "tool_use", "id": "tu_1", "name": "mia_search", "input": {}}
        ]));

        let blocks = display_blocks(&[assistant]);

        assert_eq!(blocks, vec![DisplayBlock::Text { text: "Hi".into() }]);
    }

    #[test]
    fn display_blocks_skips_tool_result_when_it_has_no_display_field() {
        init();
        let results = message_model(1, 1, MessageRole::Tool, json!([
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "found"},
            {"type": "tool_result", "tool_use_id": "tu_2", "content": "now", "mia_display": {"name": "date_time", "failed": false}}
        ]));

        let blocks = display_blocks(&[results]);

        assert_eq!(blocks, vec![DisplayBlock::Tool(date_time_view())]);
    }

    #[test]
    fn user_text_concatenates_text_blocks_when_message_has_several() {
        init();
        let message = message_model(1, 1, MessageRole::User, json!([{"type": "text", "text": "a"}, {"type": "other"}, {"type": "text", "text": "b"}]));

        assert_eq!(user_text(&message), "ab");
    }
}
