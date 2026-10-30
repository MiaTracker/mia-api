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
    Text { text: String }
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
    Completed { exchange_id: i32 },
    Failed { exchange_id: i32, key: String, debug_message: String }
}

impl ExchangeEvent {
    /// The SSE event name.
    pub fn name(&self) -> &'static str {
        match self {
            ExchangeEvent::TextDelta { .. } => "text_delta",
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
}
