//! Builds the request sent to the model from a Conversation's stored Exchanges.

use entities::sea_orm_active_enums::{ExchangeStatus, MessageRole};
use entities::{exchanges, messages};
use integrations::openrouter::{LlmMessage, LlmRequest, LlmRole};
use serde_json::json;

pub const SYSTEM_PROMPT: &str = include_str!("system_prompt.md");

/// The request for the next assistant Message of the Conversation.
///
/// `exchanges` are ordered by Exchange id, each with its Messages ordered by id. Completed
/// Exchanges and the current one are kept; all others (failed, or any other in-progress one) are
/// skipped. Content is passed through unchanged, except that the last block of the last Message
/// gets a cache marker, on the request copy only.
pub fn build_request(current_exchange_id: i32, exchanges: &[(exchanges::Model, Vec<messages::Model>)]) -> LlmRequest {
    let mut messages: Vec<LlmMessage> = exchanges
        .iter()
        .filter(|(exchange, _)| exchange.status == ExchangeStatus::Completed || exchange.id == current_exchange_id)
        .flat_map(|(_, messages)| messages)
        .map(|message| LlmMessage {
            role: match message.role {
                MessageRole::User | MessageRole::Tool => LlmRole::User,
                MessageRole::Assistant => LlmRole::Assistant,
            },
            content: message.content.clone(),
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
    LlmRequest { system: SYSTEM_PROMPT.to_owned(), messages }
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
    fn build_request_returns_no_messages_when_no_exchanges() {
        test_support::init();

        let request = build_request(1, &[]);

        assert!(request.messages.is_empty());
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
    fn build_request_uses_system_prompt() {
        test_support::init();

        let request = build_request(1, &[]);

        assert_eq!(request.system, SYSTEM_PROMPT);
        assert!(!SYSTEM_PROMPT.trim().is_empty());
    }
}
