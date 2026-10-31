//! Builds the request sent to the model from a Conversation's stored Exchanges.

use entities::sea_orm_active_enums::{ExchangeStatus, MessageRole};
use entities::{exchanges, messages};
use integrations::openrouter::{LlmMessage, LlmRequest, LlmRole};
use serde_json::{json, Value};

use crate::chat::display::DISPLAY_FIELD;
use crate::chat::tools;

pub const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

/// The request for the next assistant Message of the Conversation.
///
/// `exchanges` are ordered by Exchange id, each with its Messages ordered by id. Completed
/// Exchanges and the current one are kept; all others (failed, or any other in-progress one) are
/// skipped. Content is passed through unchanged, except that earlier Exchanges lose their thinking
/// blocks (a Message left empty by that is dropped), `tool_result` blocks lose their display field
/// and the last block of the last Message gets a cache marker, on the request copy only. Every tool
/// is offered.
pub fn build_request(current_exchange_id: i32, exchanges: &[(exchanges::Model, Vec<messages::Model>)]) -> LlmRequest {
    let mut messages: Vec<LlmMessage> = exchanges
        .iter()
        .filter(|(exchange, _)| exchange.status == ExchangeStatus::Completed || exchange.id == current_exchange_id)
        .flat_map(|(exchange, messages)| messages.iter().map(move |message| (exchange.id == current_exchange_id, message)))
        .filter_map(|(current, message)| {
            let mut content = if current { message.content.clone() } else { without_thinking(&message.content)? };
            strip_display(&mut content);
            Some(LlmMessage {
                role: match message.role {
                    MessageRole::User | MessageRole::Tool => LlmRole::User,
                    MessageRole::Assistant => LlmRole::Assistant,
                },
                content,
            })
        })
        .collect();
    let last_block = messages
        .last_mut()
        .and_then(|message| message.content.as_array_mut())
        .and_then(|blocks| blocks.last_mut())
        .and_then(|block| block.as_object_mut());
    if let Some(block) = last_block {
        block.insert("cache_control".to_owned(), json!({"type": "ephemeral"}));
    }
    LlmRequest { system: SYSTEM_PROMPT.to_owned(), messages, tools: tools::definitions() }
}

/// Removes the display field from every `tool_result` block of `content`.
fn strip_display(content: &mut Value) {
    let blocks = content.as_array_mut().into_iter().flatten();
    for block in blocks.filter(|block| block["type"] == "tool_result") {
        if let Some(block) = block.as_object_mut() {
            block.remove(DISPLAY_FIELD);
        }
    }
}

/// `content` without its `thinking` and `redacted_thinking` blocks; `None` when that removed every
/// block. Content that is not an array, or was already empty, is kept as it is.
fn without_thinking(content: &Value) -> Option<Value> {
    let Some(blocks) = content.as_array().filter(|blocks| !blocks.is_empty()) else {
        return Some(content.clone());
    };
    let kept: Vec<Value> = blocks
        .iter()
        .filter(|block| !matches!(block["type"].as_str(), Some("thinking" | "redacted_thinking")))
        .cloned()
        .collect();
    (!kept.is_empty()).then_some(Value::Array(kept))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use crate::test_support::{self, fixtures::{exchange_model, message_model}};

    fn cache_marker() -> Value {
        json!({"type": "ephemeral"})
    }

    fn text(t: &str) -> Value {
        json!([{"type": "text", "text": t}])
    }

    #[test]
    fn build_request_skips_failed_exchanges() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![
                message_model(1, 1, MessageRole::User, text("one")),
                message_model(2, 1, MessageRole::Assistant, text("reply one")),
            ]),
            (exchange_model(2, 1, ExchangeStatus::Failed), vec![message_model(3, 2, MessageRole::User, text("lost"))]),
            (exchange_model(3, 1, ExchangeStatus::InProgress), vec![message_model(4, 3, MessageRole::User, text("three"))]),
        ];

        let request = build_request(3, &exchanges);

        let texts: Vec<&str> = request.messages.iter().map(|m| m.content[0]["text"].as_str().expect("text")).collect();
        assert_eq!(texts, vec!["one", "reply one", "three"]);
    }

    #[test]
    fn build_request_includes_current_exchange_when_in_progress() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![message_model(1, 1, MessageRole::User, text("one"))]),
            (exchange_model(2, 1, ExchangeStatus::InProgress), vec![message_model(2, 2, MessageRole::User, text("two"))]),
        ];

        let request = build_request(2, &exchanges);

        assert_eq!(request.messages.len(), 2);
        assert_eq!(request.messages[1].role, LlmRole::User);
        assert_eq!(request.messages[1].content[0]["text"], "two");
    }

    #[test]
    fn build_request_skips_other_in_progress_exchange_when_not_current() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::InProgress), vec![message_model(1, 1, MessageRole::User, text("stale"))]),
            (exchange_model(2, 1, ExchangeStatus::InProgress), vec![message_model(2, 2, MessageRole::User, text("two"))]),
        ];

        let request = build_request(2, &exchanges);

        assert_eq!(request.messages.len(), 1);
        assert_eq!(request.messages[0].content[0]["text"], "two");
    }

    #[test]
    fn build_request_marks_last_block_for_caching() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![
                message_model(1, 1, MessageRole::User, text("one")),
                message_model(2, 1, MessageRole::Assistant, json!([
                    {"type": "text", "text": "first"},
                    {"type": "text", "text": "last"},
                ])),
            ]),
            (exchange_model(2, 1, ExchangeStatus::InProgress), vec![message_model(3, 2, MessageRole::User, text("two"))]),
        ];

        let request = build_request(2, &exchanges);

        assert_eq!(request.messages[2].content[0]["cache_control"], cache_marker());
        assert!(request.messages[0].content[0].get("cache_control").is_none());
        assert!(request.messages[1].content[0].get("cache_control").is_none());
        assert!(request.messages[1].content[1].get("cache_control").is_none());
        assert_eq!(request.messages[2].content[0]["text"], "two");
        // The stored Messages are untouched.
        assert!(exchanges[1].1[0].content[0].get("cache_control").is_none());
    }

    #[test]
    fn build_request_marks_only_last_block_when_last_message_has_several() {
        test_support::init();
        let exchanges = vec![(exchange_model(1, 1, ExchangeStatus::InProgress), vec![
            message_model(1, 1, MessageRole::User, json!([
                {"type": "text", "text": "a"},
                {"type": "text", "text": "b"},
            ])),
        ])];

        let request = build_request(1, &exchanges);

        assert!(request.messages[0].content[0].get("cache_control").is_none());
        assert_eq!(request.messages[0].content[1]["cache_control"], cache_marker());
    }

    #[test]
    fn build_request_adds_no_marker_when_last_content_is_empty() {
        test_support::init();
        let exchanges = vec![(exchange_model(1, 1, ExchangeStatus::InProgress), vec![
            message_model(1, 1, MessageRole::User, json!([])),
        ])];

        let request = build_request(1, &exchanges);

        assert_eq!(request.messages[0].content, json!([]));
    }

    #[test]
    fn build_request_strips_thinking_when_exchange_is_earlier() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![
                message_model(1, 1, MessageRole::User, text("one")),
                message_model(2, 1, MessageRole::Assistant, json!([
                    {"type": "thinking", "thinking": "hmm", "signature": ""},
                    {"type": "redacted_thinking", "data": "x"},
                    {"type": "text", "text": "reply"},
                ])),
            ]),
            (exchange_model(2, 1, ExchangeStatus::InProgress), vec![message_model(3, 2, MessageRole::User, text("two"))]),
        ];

        let request = build_request(2, &exchanges);

        assert_eq!(request.messages[1].content, json!([{"type": "text", "text": "reply"}]));
        assert_eq!(exchanges[0].1[1].content[0]["type"], "thinking");
    }

    #[test]
    fn build_request_keeps_thinking_when_exchange_is_current() {
        test_support::init();
        let exchanges = vec![(exchange_model(1, 1, ExchangeStatus::InProgress), vec![
            message_model(1, 1, MessageRole::User, text("q")),
            message_model(2, 1, MessageRole::Assistant, json!([
                {"type": "thinking", "thinking": "hmm", "signature": ""},
                {"type": "text", "text": "a"},
            ])),
        ])];

        let request = build_request(1, &exchanges);

        assert_eq!(request.messages[1].content[0]["type"], "thinking");
        assert_eq!(request.messages[1].content[0]["thinking"], "hmm");
    }

    #[test]
    fn build_request_drops_message_when_stripping_leaves_it_empty() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![
                message_model(1, 1, MessageRole::User, text("one")),
                message_model(2, 1, MessageRole::Assistant, json!([{"type": "thinking", "thinking": "hmm", "signature": ""}])),
            ]),
            (exchange_model(2, 1, ExchangeStatus::InProgress), vec![message_model(3, 2, MessageRole::User, text("two"))]),
        ];

        let request = build_request(2, &exchanges);

        let texts: Vec<&str> = request.messages.iter().map(|m| m.content[0]["text"].as_str().expect("text")).collect();
        assert_eq!(texts, vec!["one", "two"]);
    }

    #[test]
    fn build_request_marks_last_block_after_stripping() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![
                message_model(1, 1, MessageRole::User, text("one")),
                message_model(2, 1, MessageRole::Assistant, json!([
                    {"type": "text", "text": "reply"},
                    {"type": "thinking", "thinking": "late", "signature": ""},
                ])),
            ]),
        ];

        let request = build_request(2, &exchanges);

        assert_eq!(request.messages[1].content, json!([{"type": "text", "text": "reply", "cache_control": {"type": "ephemeral"}}]));
    }

    #[test]
    fn build_request_returns_no_messages_when_no_exchanges() {
        test_support::init();

        let request = build_request(1, &[]);

        assert!(request.messages.is_empty());
    }

    #[test]
    fn build_request_offers_every_tool_when_building_request() {
        test_support::init();

        let request = build_request(1, &[]);

        let names: Vec<&str> = request.tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(names, vec!["mia_search", "mia_details", "tmdb_search", "date_time"]);
    }

    #[test]
    fn build_request_maps_tool_role_to_user() {
        test_support::init();
        let exchanges = vec![(exchange_model(1, 1, ExchangeStatus::InProgress), vec![
            message_model(1, 1, MessageRole::User, text("q")),
            message_model(2, 1, MessageRole::Assistant, text("a")),
            message_model(3, 1, MessageRole::Tool, json!([{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}])),
        ])];

        let request = build_request(1, &exchanges);

        let roles: Vec<LlmRole> = request.messages.iter().map(|m| m.role).collect();
        assert_eq!(roles, vec![LlmRole::User, LlmRole::Assistant, LlmRole::User]);
        assert_eq!(request.messages[2].content[0]["type"], "tool_result");
    }

    #[test]
    fn build_request_strips_display_field_from_tool_results() {
        test_support::init();
        let exchanges = vec![
            (exchange_model(1, 1, ExchangeStatus::Completed), vec![
                message_model(1, 1, MessageRole::User, text("q")),
                message_model(2, 1, MessageRole::Assistant, json!([{"type": "tool_use", "id": "t1", "name": "mia_search", "input": {}}])),
                message_model(3, 1, MessageRole::Tool, json!([
                    {"type": "tool_result", "tool_use_id": "t1", "content": "boom", "is_error": true, "mia_display": {"name": "mia_search", "failed": true}}
                ])),
                message_model(4, 1, MessageRole::Assistant, text("sorry")),
            ]),
            (exchange_model(2, 1, ExchangeStatus::InProgress), vec![
                message_model(5, 2, MessageRole::User, text("again")),
                message_model(6, 2, MessageRole::Assistant, json!([{"type": "tool_use", "id": "t2", "name": "date_time", "input": {}}])),
                message_model(7, 2, MessageRole::Tool, json!([
                    {"type": "tool_result", "tool_use_id": "t2", "content": "now", "mia_display": {"name": "date_time", "failed": false}}
                ])),
            ]),
        ];

        let request = build_request(2, &exchanges);

        assert_eq!(request.messages[2].content[0]["tool_use_id"], "t1");
        assert_eq!(request.messages[2].content[0]["content"], "boom");
        assert_eq!(request.messages[2].content[0]["is_error"], true);
        assert!(request.messages[2].content[0].get("mia_display").is_none());
        assert_eq!(request.messages[6].content[0]["tool_use_id"], "t2");
        assert!(request.messages[6].content[0].get("mia_display").is_none());
        // The stored Messages are untouched.
        assert!(exchanges[0].1[2].content[0].get("mia_display").is_some());
    }

    #[test]
    fn build_request_uses_system_prompt() {
        test_support::init();

        let request = build_request(1, &[]);

        assert_eq!(request.system, SYSTEM_PROMPT);
        assert!(!SYSTEM_PROMPT.trim().is_empty());
    }
}
