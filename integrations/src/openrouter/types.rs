use serde_json::Value;

/// Who authored a message sent to the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmRole {
    User,
    Assistant,
}

/// One message of the conversation sent to the model.
#[derive(Clone, Debug, PartialEq)]
pub struct LlmMessage {
    pub role: LlmRole,
    /// An array of Anthropic content blocks.
    pub content: Value,
}

/// A request for one assistant message.
#[derive(Clone, Debug, PartialEq)]
pub struct LlmRequest {
    pub system: String,
    pub messages: Vec<LlmMessage>,
}

/// The kind of a content block the model starts.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Text,
    Thinking,
    /// Any other block type, with the provider's name for it.
    Other(String),
}

/// Why the model stopped generating.
#[derive(Clone, Debug, PartialEq)]
pub enum StopReason {
    EndTurn,
    StopSequence,
    MaxTokens,
    ToolUse,
    Refusal,
    /// Any other stop reason, with the provider's value.
    Other(String),
}

impl StopReason {
    /// The Anthropic wire value, e.g. `end_turn`.
    pub fn as_str(&self) -> &str {
        match self {
            StopReason::EndTurn => "end_turn",
            StopReason::StopSequence => "stop_sequence",
            StopReason::MaxTokens => "max_tokens",
            StopReason::ToolUse => "tool_use",
            StopReason::Refusal => "refusal",
            StopReason::Other(other) => other,
        }
    }
}

/// Token counts reported by the provider; each is absent when not reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LlmUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
}

/// One event of a streamed assistant message.
#[derive(Clone, Debug, PartialEq)]
pub enum LlmEvent {
    MessageStart { request_id: Option<String>, usage: LlmUsage },
    BlockStart { index: u32, kind: BlockKind },
    TextDelta { index: u32, text: String },
    ThinkingDelta { index: u32, thinking: String },
    SignatureDelta { index: u32, signature: String },
    BlockStop { index: u32 },
    MessageDelta { stop_reason: Option<StopReason>, usage: LlmUsage },
    MessageStop,
}

/// The `error_type` of an error we raise ourselves when a request cannot be serialized or a
/// stream frame cannot be parsed. Retrying cannot help, so it is no transient failure.
pub const UNPARSEABLE_RESPONSE: &str = "unparseable_response";

/// A failed request, before or during streaming.
#[derive(Clone, Debug, PartialEq)]
pub struct LlmError {
    /// HTTP status of a pre-stream error response.
    pub status: Option<u16>,
    /// OpenRouter's canonical error type, set for mid-stream errors; or [`UNPARSEABLE_RESPONSE`]
    /// when a request or stream frame could not be (de)serialized.
    pub error_type: Option<String>,
    pub message: String,
    /// The raw provider JSON.
    pub response: Option<Value>,
}
