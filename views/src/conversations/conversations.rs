use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Conversation {
    pub id: i32,
    pub title: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTime<Utc>
}

#[derive(Serialize, ToSchema)]
pub struct ConversationDetails {
    pub id: i32,
    pub title: Option<String>,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = String, format = DateTime)]
    pub updated_at: DateTime<Utc>,
    pub exchanges: Vec<ExchangeView>
}

#[derive(Serialize, ToSchema, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeStatusView {
    InProgress,
    Completed,
    Failed
}

#[derive(Serialize, ToSchema)]
pub struct ExchangeView {
    pub id: i32,
    pub status: ExchangeStatusView,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime<Utc>,
    #[schema(value_type = Option<String>, format = DateTime)]
    pub completed_at: Option<DateTime<Utc>>,
    pub content: String,
    pub blocks: Vec<DisplayBlock>,
    pub error: Option<ExchangeError>
}

#[derive(Serialize, ToSchema, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DisplayBlock {
    Text { text: String },
    Thinking { thinking: String }
}

#[derive(Deserialize, ToSchema)]
pub struct ExchangeCreate {
    pub content: String
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug, PartialEq)]
pub struct ExchangeError {
    pub key: String,
    pub debug_message: String
}

/// The data of one server-sent event of an Exchange stream.
#[derive(Serialize, ToSchema, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum ExchangeEvent {
    TextDelta { block: u32, text: String },
    ThinkingDelta { block: u32, text: String },
    Completed { exchange_id: i32 },
    Failed { exchange_id: i32, key: String, debug_message: String }
}

impl ExchangeEvent {
    /// The delta that replays the whole of `shown`, block number `block`.
    pub fn replay(block: u32, shown: DisplayBlock) -> Self {
        match shown {
            DisplayBlock::Text { text } => ExchangeEvent::TextDelta { block, text },
            DisplayBlock::Thinking { thinking } => ExchangeEvent::ThinkingDelta { block, text: thinking }
        }
    }

    /// The SSE event name.
    pub fn name(&self) -> &'static str {
        match self {
            ExchangeEvent::TextDelta { .. } => "text_delta",
            ExchangeEvent::ThinkingDelta { .. } => "thinking_delta",
            ExchangeEvent::Completed { .. } => "exchange_completed",
            ExchangeEvent::Failed { .. } => "exchange_failed"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn exchange_event_serializes_without_tag_when_text_delta() {
        let event = ExchangeEvent::TextDelta { block: 0, text: "Hi".into() };

        assert_eq!(serde_json::to_value(&event).unwrap(), json!({"block": 0, "text": "Hi"}));
        assert_eq!(event.name(), "text_delta");
    }

    #[test]
    fn exchange_event_serializes_block_and_text_when_thinking_delta() {
        let event = ExchangeEvent::ThinkingDelta { block: 0, text: "Hm".into() };

        assert_eq!(serde_json::to_value(&event).unwrap(), json!({"block": 0, "text": "Hm"}));
        assert_eq!(event.name(), "thinking_delta");
    }

    #[test]
    fn display_block_serializes_with_thinking_type_when_thinking() {
        let block = DisplayBlock::Thinking { thinking: "Hm".into() };

        assert_eq!(serde_json::to_value(&block).unwrap(), json!({"type": "thinking", "thinking": "Hm"}));
    }

    #[test]
    fn replay_returns_thinking_delta_when_block_is_thinking() {
        let event = ExchangeEvent::replay(3, DisplayBlock::Thinking { thinking: "Hm".into() });

        assert_eq!(event, ExchangeEvent::ThinkingDelta { block: 3, text: "Hm".into() });
    }

    #[test]
    fn replay_returns_text_delta_when_block_is_text() {
        let event = ExchangeEvent::replay(1, DisplayBlock::Text { text: "Hi".into() });

        assert_eq!(event, ExchangeEvent::TextDelta { block: 1, text: "Hi".into() });
    }
}
