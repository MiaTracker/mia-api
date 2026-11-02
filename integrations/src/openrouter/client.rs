use crate::openrouter::types::{
    BlockKind, DataPolicy, LlmError, LlmEvent, LlmMessage, LlmRequest, LlmRole, LlmUsage, ReasoningEffort, StopReason, UNPARSEABLE_RESPONSE,
};
use async_trait::async_trait;
use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use openrouter_rs::OpenRouterClient as RsClient;
use openrouter_rs::api::messages::{
    AnthropicContentPart, AnthropicMessage, AnthropicMessageContent, AnthropicMessagesRequest,
    AnthropicMessagesStreamEvent, AnthropicOutputConfig, AnthropicOutputEffort, AnthropicRole, AnthropicSystemPrompt, AnthropicThinking, AnthropicTool,
};
use openrouter_rs::error::OpenRouterError;
use openrouter_rs::types::{DataCollectionPolicy, ProviderPreferences};
use serde_json::Value;

/// The events of one streamed assistant message. An `Err` item ends the stream.
pub type LlmEventStream = BoxStream<'static, Result<LlmEvent, LlmError>>;

/// Every LLM operation the application uses.
#[async_trait]
pub trait Llm: Send + Sync {
    /// The model requests are sent to.
    fn model(&self) -> String;
    /// Starts streaming the model's answer to `request`. Fails with the provider's error
    /// when it rejects the request before streaming.
    async fn stream_message(&self, request: LlmRequest) -> Result<LlmEventStream, LlmError>;
}

/// [`Llm`] backed by OpenRouter's Anthropic-compatible Messages endpoint.
pub struct OpenRouterClient {
    client: RsClient,
    model: String,
    max_tokens: u32,
    /// How hard the model is asked to think; `None` disables reasoning.
    reasoning_effort: Option<ReasoningEffort>,
    data_policy: DataPolicy,
}

impl OpenRouterClient {
    pub fn new(
        api_key: String,
        model: String,
        max_tokens: u32,
        reasoning_effort: Option<ReasoningEffort>,
        data_policy: DataPolicy,
    ) -> Self {
        let client = RsClient::builder()
            .api_key(api_key)
            .build()
            .expect("OpenRouter client builds from an API key");
        Self { client, model, max_tokens, reasoning_effort, data_policy }
    }

    /// The provider routing preferences for the data policy; `None` when any provider is allowed.
    fn provider_preferences(&self) -> Option<ProviderPreferences> {
        let mut preferences = ProviderPreferences::default();
        match self.data_policy {
            DataPolicy::Allow => return None,
            DataPolicy::Deny => preferences.data_collection = Some(DataCollectionPolicy::Deny),
            DataPolicy::Zdr => preferences.zdr = Some(true),
        }
        Some(preferences)
    }

    fn build_request(&self, request: LlmRequest) -> Result<AnthropicMessagesRequest, LlmError> {
        let mut messages = Vec::with_capacity(request.messages.len());
        for message in request.messages {
            messages.push(translate_message(message)?);
        }
        let mut builder = AnthropicMessagesRequest::builder();
        builder
            .model(self.model.clone())
            .max_tokens(self.max_tokens)
            .messages(messages);
        match self.reasoning_effort {
            Some(effort) => {
                builder
                    .thinking(AnthropicThinking::Adaptive)
                    .output_config(AnthropicOutputConfig::with_effort(output_effort(effort)));
            }
            None => {
                builder.thinking(AnthropicThinking::Disabled);
            }
        }
        if let Some(preferences) = self.provider_preferences() {
            builder.provider(preferences);
        }
        if !request.system.is_empty() {
            builder.system(AnthropicSystemPrompt::Text(request.system));
        }
        for tool in request.tools {
            let mut tool = AnthropicTool::custom(tool.name, tool.description, tool.input_schema);
            // A client-side tool is sent in the plain Anthropic shape, without the `custom` type tag.
            tool.tool_type = None;
            builder.tool(tool);
        }
        builder.build().map_err(translate_error)
    }
}

#[async_trait]
impl Llm for OpenRouterClient {
    fn model(&self) -> String {
        self.model.clone()
    }

    async fn stream_message(&self, request: LlmRequest) -> Result<LlmEventStream, LlmError> {
        let request = self.build_request(request)?;
        let stream = self.client.stream_messages(&request).await.map_err(translate_error)?;
        Ok(stream
            .flat_map(|item| {
                let events = match item {
                    Ok(sse) => translate_event(sse.data),
                    Err(error) => vec![Err(translate_error(error))],
                };
                futures_util::stream::iter(events)
            })
            .boxed())
    }
}

fn output_effort(effort: ReasoningEffort) -> AnthropicOutputEffort {
    match effort {
        ReasoningEffort::Low => AnthropicOutputEffort::Low,
        ReasoningEffort::Medium => AnthropicOutputEffort::Medium,
        ReasoningEffort::High => AnthropicOutputEffort::High,
        ReasoningEffort::Xhigh => AnthropicOutputEffort::Xhigh,
        ReasoningEffort::Max => AnthropicOutputEffort::Max,
    }
}

fn translate_message(message: LlmMessage) -> Result<AnthropicMessage, LlmError> {
    let role = match message.role {
        LlmRole::User => AnthropicRole::User,
        LlmRole::Assistant => AnthropicRole::Assistant,
    };
    let content: AnthropicMessageContent = serde_json::from_value(message.content)
        .map_err(|error| translate_error(OpenRouterError::Serialization(error)))?;
    Ok(AnthropicMessage::new(role, content))
}

fn translate_error(error: OpenRouterError) -> LlmError {
    match error {
        OpenRouterError::Api(context) => LlmError {
            status: Some(context.status.as_u16()),
            error_type: None,
            message: context.message,
            response: context.metadata,
        },
        // Retrying cannot help: the same request or frame fails to parse the same way again.
        OpenRouterError::Serialization(_) => LlmError {
            status: None,
            error_type: Some(UNPARSEABLE_RESPONSE.to_owned()),
            message: error.to_string(),
            response: None,
        },
        other => LlmError { status: None, error_type: None, message: other.to_string(), response: None },
    }
}

/// The events for one stream frame: none for a ping or an unknown delta, two or more for a block
/// that starts with content already in it.
fn translate_event(event: AnthropicMessagesStreamEvent) -> Vec<Result<LlmEvent, LlmError>> {
    let event = match event {
        AnthropicMessagesStreamEvent::Ping => return Vec::new(),
        AnthropicMessagesStreamEvent::Error { error } => {
            return vec![Err(LlmError {
                status: None,
                error_type: error["error_type"].as_str().map(str::to_owned),
                message: error["message"].as_str().unwrap_or_default().to_owned(),
                response: Some(error),
            })];
        }
        AnthropicMessagesStreamEvent::MessageStart { message } => {
            let usage = message.usage.as_ref();
            LlmEvent::MessageStart {
                request_id: message.id.clone(),
                usage: LlmUsage {
                    input_tokens: usage.and_then(|u| u.input_tokens),
                    output_tokens: usage.and_then(|u| u.output_tokens),
                    cache_read_tokens: usage.and_then(|u| u.cache_read_input_tokens),
                    cache_creation_tokens: usage.and_then(|u| u.cache_creation_input_tokens),
                },
            }
        }
        AnthropicMessagesStreamEvent::ContentBlockStart { index, content_block } => {
            return translate_block_start(index, *content_block);
        }
        AnthropicMessagesStreamEvent::ContentBlockDelta { index, delta } => {
            let field = |name: &str| delta[name].as_str().unwrap_or_default().to_owned();
            match delta["type"].as_str() {
                Some("text_delta") => LlmEvent::TextDelta { index, text: field("text") },
                Some("thinking_delta") => LlmEvent::ThinkingDelta { index, thinking: field("thinking") },
                Some("signature_delta") => LlmEvent::SignatureDelta { index, signature: field("signature") },
                Some("input_json_delta") => match field("partial_json") {
                    partial_json if partial_json.is_empty() => return Vec::new(),
                    partial_json => LlmEvent::InputJsonDelta { index, partial_json },
                },
                _ => return Vec::new(),
            }
        }
        AnthropicMessagesStreamEvent::ContentBlockStop { index } => LlmEvent::BlockStop { index },
        AnthropicMessagesStreamEvent::MessageDelta { delta, usage } => LlmEvent::MessageDelta {
            stop_reason: delta["stop_reason"].as_str().map(stop_reason),
            usage: LlmUsage {
                input_tokens: usage["input_tokens"].as_u64(),
                output_tokens: usage["output_tokens"].as_u64(),
                cache_read_tokens: usage["cache_read_input_tokens"].as_u64(),
                cache_creation_tokens: usage["cache_creation_input_tokens"].as_u64(),
            },
        },
        AnthropicMessagesStreamEvent::MessageStop { .. } => LlmEvent::MessageStop,
        _ => return Vec::new(),
    };
    vec![Ok(event)]
}

/// `BlockStart`, followed by deltas for any content the block starts with, so that content is
/// handled like the content streamed after it.
fn translate_block_start(index: u32, block: AnthropicContentPart) -> Vec<Result<LlmEvent, LlmError>> {
    let (kind, initial) = match block {
        AnthropicContentPart::Text { text, .. } => {
            (BlockKind::Text, vec![LlmEvent::TextDelta { index, text }])
        }
        AnthropicContentPart::Thinking { thinking, signature } => (
            BlockKind::Thinking,
            vec![
                LlmEvent::ThinkingDelta { index, thinking },
                LlmEvent::SignatureDelta { index, signature },
            ],
        ),
        AnthropicContentPart::ToolUse { id, name, input, .. } => {
            // A streamed call starts with an empty input; its JSON follows as deltas.
            let initial = match input {
                Some(Value::Object(object)) if !object.is_empty() => {
                    vec![LlmEvent::InputJsonDelta { index, partial_json: Value::Object(object).to_string() }]
                }
                _ => Vec::new(),
            };
            (BlockKind::ToolUse { id, name }, initial)
        }
        AnthropicContentPart::RedactedThinking { data } => (BlockKind::RedactedThinking { data }, Vec::new()),
        other => {
            let value = serde_json::to_value(&other).unwrap_or_default();
            (BlockKind::Other(value["type"].as_str().unwrap_or_default().to_owned()), Vec::new())
        }
    };
    let initial = initial.into_iter().filter(|event| match event {
        LlmEvent::TextDelta { text, .. } => !text.is_empty(),
        LlmEvent::ThinkingDelta { thinking, .. } => !thinking.is_empty(),
        LlmEvent::SignatureDelta { signature, .. } => !signature.is_empty(),
        _ => true,
    });
    std::iter::once(LlmEvent::BlockStart { index, kind }).chain(initial).map(Ok).collect()
}

fn stop_reason(value: &str) -> StopReason {
    match value {
        "end_turn" => StopReason::EndTurn,
        "stop_sequence" => StopReason::StopSequence,
        "max_tokens" => StopReason::MaxTokens,
        "tool_use" => StopReason::ToolUse,
        "refusal" => StopReason::Refusal,
        other => StopReason::Other(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openrouter::types::LlmTool;
    use openrouter_rs::error::ApiErrorContext;
    use serde_json::json;

    fn event(value: Value) -> AnthropicMessagesStreamEvent {
        serde_json::from_value(value).unwrap()
    }

    fn block_start(content_block: Value) -> Value {
        json!({"type": "content_block_start", "index": 1, "content_block": content_block})
    }

    fn user(content: Value) -> LlmMessage {
        LlmMessage { role: LlmRole::User, content }
    }

    #[test]
    fn translate_event_returns_text_delta_when_delta_is_text() {
        let result = translate_event(event(json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "text_delta", "text": "Hello"}
        })));

        assert_eq!(result, vec![Ok(LlmEvent::TextDelta { index: 2, text: "Hello".into() })]);
    }

    #[test]
    fn translate_event_returns_error_with_error_type_when_stream_sends_error() {
        let raw = json!({"error_type": "rate_limit_exceeded", "message": "slow down"});

        let result = translate_event(event(json!({"type": "error", "error": raw.clone()})));

        assert_eq!(
            result,
            vec![Err(LlmError {
                status: None,
                error_type: Some("rate_limit_exceeded".into()),
                message: "slow down".into(),
                response: Some(raw),
            })]
        );
    }

    #[test]
    fn translate_event_returns_stop_reason_and_output_tokens_when_message_delta() {
        let result = translate_event(event(json!({
            "type": "message_delta",
            "delta": {"stop_reason": "max_tokens"},
            "usage": {"output_tokens": 64}
        })));

        assert_eq!(
            result,
            vec![Ok(LlmEvent::MessageDelta {
                stop_reason: Some(StopReason::MaxTokens),
                usage: LlmUsage { output_tokens: Some(64), ..LlmUsage::default() },
            })]
        );
    }

    #[test]
    fn translate_event_skips_ping() {
        assert_eq!(translate_event(event(json!({"type": "ping"}))), vec![]);
    }

    #[test]
    fn translate_event_returns_block_start_only_when_text_block_starts_empty() {
        let result = translate_event(event(block_start(json!({"type": "text", "text": ""}))));

        assert_eq!(result, vec![Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::Text })]);
    }

    #[test]
    fn translate_event_returns_block_start_then_text_delta_when_text_block_starts_with_text() {
        let result = translate_event(event(block_start(json!({"type": "text", "text": "Hel"}))));

        assert_eq!(
            result,
            vec![
                Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::Text }),
                Ok(LlmEvent::TextDelta { index: 1, text: "Hel".into() }),
            ]
        );
    }

    #[test]
    fn translate_event_returns_thinking_block_start_when_thinking_block_has_empty_signature() {
        let result = translate_event(event(block_start(json!({"type": "thinking", "thinking": "", "signature": ""}))));

        assert_eq!(result, vec![Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::Thinking })]);
    }

    #[test]
    fn translate_event_returns_thinking_and_signature_deltas_when_thinking_block_starts_with_content() {
        let result =
            translate_event(event(block_start(json!({"type": "thinking", "thinking": "hmm", "signature": "sig"}))));

        assert_eq!(
            result,
            vec![
                Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::Thinking }),
                Ok(LlmEvent::ThinkingDelta { index: 1, thinking: "hmm".into() }),
                Ok(LlmEvent::SignatureDelta { index: 1, signature: "sig".into() }),
            ]
        );
    }

    #[test]
    fn translate_event_returns_redacted_thinking_block_start_when_redacted_thinking_starts() {
        let result = translate_event(event(block_start(json!({"type": "redacted_thinking", "data": "x"}))));

        assert_eq!(
            result,
            vec![Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::RedactedThinking { data: "x".into() } })]
        );
    }

    #[test]
    fn translate_event_returns_other_block_start_when_block_type_unhandled() {
        let result = translate_event(event(block_start(
            json!({"type": "server_tool_use", "id": "s_1", "name": "web_search", "input": {}}),
        )));

        assert_eq!(
            result,
            vec![Ok(LlmEvent::BlockStart { index: 1, kind: BlockKind::Other("server_tool_use".into()) })]
        );
    }

    #[test]
    fn translate_event_returns_tool_use_block_start_when_tool_use_block_starts() {
        let result = translate_event(event(block_start(
            json!({"type": "tool_use", "id": "tu_1", "name": "date_time", "input": {}}),
        )));

        assert_eq!(
            result,
            vec![Ok(LlmEvent::BlockStart {
                index: 1,
                kind: BlockKind::ToolUse { id: "tu_1".into(), name: "date_time".into() }
            })]
        );
    }

    #[test]
    fn translate_event_returns_input_json_delta_when_tool_use_block_starts_with_input() {
        let result = translate_event(event(block_start(
            json!({"type": "tool_use", "id": "tu_1", "name": "tmdb_search", "input": {"query": "x"}}),
        )));

        assert_eq!(
            result,
            vec![
                Ok(LlmEvent::BlockStart {
                    index: 1,
                    kind: BlockKind::ToolUse { id: "tu_1".into(), name: "tmdb_search".into() }
                }),
                Ok(LlmEvent::InputJsonDelta { index: 1, partial_json: r#"{"query":"x"}"#.into() }),
            ]
        );
    }

    #[test]
    fn translate_event_skips_input_json_delta_when_partial_json_is_empty() {
        let result = translate_event(event(json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "input_json_delta", "partial_json": ""}
        })));

        assert_eq!(result, vec![]);
    }

    #[test]
    fn translate_event_returns_input_json_delta_when_delta_is_input_json() {
        let result = translate_event(event(json!({
            "type": "content_block_delta",
            "index": 2,
            "delta": {"type": "input_json_delta", "partial_json": "{\"q"}
        })));

        assert_eq!(result, vec![Ok(LlmEvent::InputJsonDelta { index: 2, partial_json: "{\"q".into() })]);
    }

    /// Known gap: openrouter-rs's typed `Thinking` block requires `signature`, so it fails to parse
    /// a thinking `content_block_start` frame without one before our code sees it. The stream then
    /// yields an `Err` item, which ends the call as `ProviderError`.
    #[test]
    fn translate_error_marks_unparseable_when_thinking_block_start_has_no_signature() {
        let frame = block_start(json!({"type": "thinking", "thinking": ""}));

        let parsed = serde_json::from_value::<AnthropicMessagesStreamEvent>(frame);

        let error = translate_error(OpenRouterError::Serialization(parsed.expect_err("frame without signature parsed")));
        assert_eq!(error.status, None);
        assert_eq!(error.error_type.as_deref(), Some(UNPARSEABLE_RESPONSE));
        assert!(error.message.starts_with("Serialization error"), "message: {}", error.message);
    }

    #[test]
    fn translate_message_accepts_text_block_when_it_carries_cache_control() {
        let content = json!([{"type": "text", "text": "Hi", "cache_control": {"type": "ephemeral"}}]);

        let message = translate_message(user(content.clone())).expect("translated");

        assert_eq!(serde_json::to_value(&message.content).unwrap(), content);
    }

    #[test]
    fn translate_message_accepts_thinking_block_when_signature_is_empty() {
        let content = json!([
            {"type": "thinking", "thinking": "hmm", "signature": ""},
            {"type": "text", "text": "Hello", "cache_control": {"type": "ephemeral"}}
        ]);

        let message = translate_message(LlmMessage { role: LlmRole::Assistant, content: content.clone() }).expect("translated");

        assert_eq!(serde_json::to_value(&message.content).unwrap(), content);
    }

    #[test]
    fn translate_message_accepts_tool_use_and_tool_result_blocks() {
        let tool_use = json!([{"type": "tool_use", "id": "tu_1", "name": "date_time", "input": {}}]);
        let tool_result =
            json!([{"type": "tool_result", "tool_use_id": "tu_1", "content": "{\"now\":\"2026-10-31\"}", "is_error": true}]);

        let assistant = translate_message(LlmMessage { role: LlmRole::Assistant, content: tool_use.clone() });
        let user_message = translate_message(user(tool_result.clone()));

        assert_eq!(serde_json::to_value(&assistant.expect("translated").content).unwrap(), tool_use);
        assert_eq!(serde_json::to_value(&user_message.expect("translated").content).unwrap(), tool_result);
    }

    #[test]
    fn translate_message_fails_as_unparseable_when_content_is_not_content_blocks() {
        let error = translate_message(user(json!([{"type": "text"}]))).expect_err("translated");

        assert_eq!(error.error_type.as_deref(), Some(UNPARSEABLE_RESPONSE));
    }

    fn request_json(reasoning_effort: Option<ReasoningEffort>, tools: Vec<LlmTool>) -> Value {
        request_json_with_policy(reasoning_effort, tools, DataPolicy::Allow)
    }

    fn request_json_with_policy(reasoning_effort: Option<ReasoningEffort>, tools: Vec<LlmTool>, data_policy: DataPolicy) -> Value {
        let client = OpenRouterClient::new("key".into(), "test/model".into(), 4096, reasoning_effort, data_policy);
        let request = client.build_request(LlmRequest {
            system: String::new(),
            messages: vec![user(json!([{"type": "text", "text": "Hi"}]))],
            tools,
        });
        serde_json::to_value(request.expect("built")).unwrap()
    }

    #[test]
    fn build_request_disables_thinking_when_no_effort() {
        let json = request_json(None, Vec::new());

        assert_eq!(json["thinking"], json!({"type": "disabled"}));
        assert!(json.get("output_config").is_none());
    }

    #[test]
    fn build_request_sends_adaptive_thinking_with_effort_when_effort_set() {
        for (effort, text) in [
            (ReasoningEffort::Low, "low"),
            (ReasoningEffort::Medium, "medium"),
            (ReasoningEffort::High, "high"),
            (ReasoningEffort::Xhigh, "xhigh"),
            (ReasoningEffort::Max, "max"),
        ] {
            let json = request_json(Some(effort), Vec::new());

            assert_eq!(json["thinking"], json!({"type": "adaptive"}));
            assert_eq!(json["output_config"], json!({"effort": text}));
        }
    }

    #[test]
    fn build_request_sends_tools_without_type_when_tools_given() {
        let tool = LlmTool { name: "date_time".into(), description: "d".into(), input_schema: json!({"type": "object"}) };

        let with_tools = request_json(None, vec![tool]);
        let without_tools = request_json(None, Vec::new());

        assert_eq!(with_tools["tools"], json!([{"name": "date_time", "description": "d", "input_schema": {"type": "object"}}]));
        assert!(without_tools.get("tools").is_none());
    }

    #[test]
    fn build_request_sends_no_provider_when_policy_allow() {
        assert!(request_json_with_policy(None, Vec::new(), DataPolicy::Allow).get("provider").is_none());
    }

    #[test]
    fn build_request_denies_data_collection_when_policy_deny() {
        let request = request_json_with_policy(None, Vec::new(), DataPolicy::Deny);

        assert_eq!(request["provider"], json!({"data_collection": "deny"}));
    }

    #[test]
    fn build_request_requires_zdr_when_policy_zdr() {
        let request = request_json_with_policy(None, Vec::new(), DataPolicy::Zdr);

        assert_eq!(request["provider"], json!({"zdr": true}));
    }

    #[test]
    fn translate_error_keeps_status_when_api_error() {
        let error = OpenRouterError::Api(Box::new(ApiErrorContext {
            status: reqwest::StatusCode::PAYMENT_REQUIRED,
            api_code: None,
            message: "insufficient credits".into(),
            request_id: None,
            metadata: Some(json!({"detail": 1})),
            kind: openrouter_rs::error::ApiErrorKind::Generic,
        }));

        let result = translate_error(error);

        assert_eq!(
            result,
            LlmError {
                status: Some(402),
                error_type: None,
                message: "insufficient credits".into(),
                response: Some(json!({"detail": 1})),
            }
        );
    }
}
