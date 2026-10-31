//! Contract test against the real OpenRouter API. It needs a key in `config.yaml`
//! (`openrouter.api_key`) and is ignored by default. It uses the free router, so it costs nothing.
//!
//! Run with `cargo test -p integrations --test openrouter_contract -- --ignored`.

use futures_util::StreamExt;
use integrations::openrouter::{BlockKind, Llm, LlmEvent, LlmMessage, LlmRequest, LlmRole, LlmTool, OpenRouterClient};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct TestConfig {
    openrouter: TestOpenRouterConfig,
}

#[derive(Deserialize)]
struct TestOpenRouterConfig {
    api_key: String,
}

fn client(max_tokens: u32, thinking_budget: Option<u32>) -> OpenRouterClient {
    client_for("openrouter/free", max_tokens, thinking_budget)
}

fn client_for(model: &str, max_tokens: u32, thinking_budget: Option<u32>) -> OpenRouterClient {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.yaml");
    let config: TestConfig = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_yaml::from_str(&text).ok())
        .expect("config.yaml with openrouter.api_key required for OpenRouter contract tests");
    OpenRouterClient::new(config.openrouter.api_key, model.to_string(), max_tokens, thinking_budget)
}

fn say_hello() -> LlmRequest {
    LlmRequest {
        system: "Answer in one short sentence.".to_string(),
        messages: vec![LlmMessage {
            role: LlmRole::User,
            content: json!([{"type": "text", "text": "Say hello."}]),
        }],
        tools: Vec::new(),
    }
}

async fn stream_events(client: OpenRouterClient) -> Vec<LlmEvent> {
    let mut stream = client.stream_message(say_hello()).await.unwrap();
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream yielded an error"));
    }
    events
}

#[tokio::test]
#[ignore = "requires OpenRouter key"]
async fn stream_message_yields_text_and_stop_reason_when_prompt_is_short() {
    let events = stream_events(client(512, None)).await;

    assert!(events
        .iter()
        .any(|e| matches!(e, LlmEvent::TextDelta { text, .. } if !text.is_empty())));
    assert!(events
        .iter()
        .any(|e| matches!(e, LlmEvent::MessageDelta { stop_reason: Some(_), .. })));
}

/// Reasoning is requested, but the free router may pick a model that doesn't reason, so thinking
/// is counted, not required.
#[tokio::test]
#[ignore = "requires OpenRouter key"]
async fn stream_message_reaches_stop_reason_when_reasoning_enabled() {
    let events = stream_events(client(2048, Some(1024))).await;

    let thinking = events
        .iter()
        .filter(|e| matches!(e, LlmEvent::ThinkingDelta { thinking, .. } if !thinking.is_empty()))
        .count();
    eprintln!("non-empty thinking deltas: {thinking}");
    assert!(events
        .iter()
        .any(|e| matches!(e, LlmEvent::MessageDelta { stop_reason: Some(_), .. })));
}

/// Runs on a pinned free model that advertises tool support: `openrouter/free` may route to a model
/// that ignores the tool or rejects the request, and other free tool models were rate limited
/// (gemma-4-31b-it) or erroring upstream (nemotron-3-ultra). Replace the model if OpenRouter retires it.
const TOOL_MODEL: &str = "poolside/laguna-s-2.1:free";

fn weather_request() -> LlmRequest {
    LlmRequest {
        system: "Use the tool to answer. Once you have the tool result, answer in one short sentence.".to_string(),
        messages: vec![LlmMessage {
            role: LlmRole::User,
            content: json!([{"type": "text", "text": "What is the weather in Paris? Use the get_weather tool."}]),
        }],
        tools: vec![LlmTool {
            name: "get_weather".to_string(),
            description: "Gets the current weather of a city.".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"]
            }),
        }],
    }
}

async fn stream_request(client: &OpenRouterClient, request: LlmRequest) -> Vec<LlmEvent> {
    let mut stream = client.stream_message(request).await.unwrap();
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream yielded an error"));
    }
    events
}

/// The assistant content blocks of `events`, built the way mia's accumulator stores them: blocks
/// in start order; text and thinking only when non-empty, thinking with whatever signature
/// arrived (possibly empty); tool_use with its parsed input; redacted_thinking with its data.
fn assistant_content(events: &[LlmEvent]) -> Vec<serde_json::Value> {
    let mut blocks: Vec<(u32, serde_json::Value)> = Vec::new();
    let mut json_input: Vec<(u32, String)> = Vec::new();
    for event in events {
        match event {
            LlmEvent::BlockStart { index, kind } => blocks.push((*index, match kind {
                BlockKind::Text => json!({"type": "text", "text": ""}),
                BlockKind::Thinking => json!({"type": "thinking", "thinking": "", "signature": ""}),
                BlockKind::ToolUse { id, name } => {
                    json_input.push((*index, String::new()));
                    json!({"type": "tool_use", "id": id, "name": name, "input": {}})
                }
                BlockKind::RedactedThinking { data } => json!({"type": "redacted_thinking", "data": data}),
                BlockKind::Other(other) => json!({"type": other}),
            })),
            LlmEvent::TextDelta { index, text } => append(&mut blocks, *index, "text", text),
            LlmEvent::ThinkingDelta { index, thinking } => append(&mut blocks, *index, "thinking", thinking),
            LlmEvent::SignatureDelta { index, signature } => append(&mut blocks, *index, "signature", signature),
            LlmEvent::InputJsonDelta { index, partial_json } => {
                if let Some((_, json)) = json_input.iter_mut().find(|(i, _)| i == index) {
                    json.push_str(partial_json);
                }
            }
            _ => {}
        }
    }
    for (index, json) in &json_input {
        if !json.trim().is_empty() {
            let input: serde_json::Value = serde_json::from_str(json).expect("tool input is not valid JSON");
            blocks.iter_mut().find(|(i, _)| i == index).expect("tool block").1["input"] = input;
        }
    }
    blocks
        .into_iter()
        .map(|(_, block)| block)
        .filter(|block| match block["type"].as_str() {
            Some("text") => block["text"] != "",
            Some("thinking") => block["thinking"] != "",
            Some("tool_use" | "redacted_thinking") => true,
            _ => false,
        })
        .collect()
}

fn append(blocks: &mut [(u32, serde_json::Value)], index: u32, field: &str, text: &str) {
    if let Some((_, block)) = blocks.iter_mut().find(|(i, _)| *i == index) {
        let joined = format!("{}{text}", block[field].as_str().unwrap_or_default());
        block[field] = json!(joined);
    }
}

#[tokio::test]
#[ignore = "requires OpenRouter key"]
async fn stream_message_yields_tool_use_with_parseable_input_when_tool_is_offered() {
    let events = stream_request(&client_for(TOOL_MODEL, 2048, Some(1024)), weather_request()).await;

    let index = events
        .iter()
        .find_map(|e| match e {
            LlmEvent::BlockStart { index, kind: BlockKind::ToolUse { name, .. } } if name == "get_weather" => Some(*index),
            _ => None,
        })
        .expect("no get_weather tool_use block started");
    let input: String = events
        .iter()
        .filter_map(|e| match e {
            LlmEvent::InputJsonDelta { index: i, partial_json } if *i == index => Some(partial_json.as_str()),
            _ => None,
        })
        .collect();
    let parsed: serde_json::Value = serde_json::from_str(&input).expect("tool input is not valid JSON");
    assert!(parsed.is_object(), "tool input is not an object: {input}");
}

/// The second call of a tool round, sent as mia sends it: the streamed assistant blocks (thinking
/// with whatever signature arrived, redacted_thinking, tool_use) and then a user message with the
/// matching string tool_result, its last block cache-marked. The model must answer with text.
#[tokio::test]
#[ignore = "requires OpenRouter key"]
async fn stream_message_answers_with_text_when_tool_result_follows_streamed_tool_use() {
    let client = client_for(TOOL_MODEL, 2048, Some(1024));
    let first = stream_request(&client, weather_request()).await;
    let assistant = assistant_content(&first);
    let tool_use_id = assistant
        .iter()
        .find(|block| block["type"] == "tool_use" && block["name"] == "get_weather")
        .and_then(|block| block["id"].as_str())
        .expect("no get_weather tool_use block")
        .to_string();
    let signatures: Vec<usize> = assistant
        .iter()
        .filter(|block| block["type"] == "thinking")
        .map(|block| block["signature"].as_str().map_or(0, str::len))
        .collect();
    let redacted = assistant.iter().filter(|block| block["type"] == "redacted_thinking").count();
    eprintln!("first call: thinking blocks with signature lengths {signatures:?}, redacted_thinking blocks: {redacted}");
    eprintln!("assistant content sent back: {}", serde_json::Value::Array(assistant.clone()));

    let mut request = weather_request();
    request.messages.push(LlmMessage { role: LlmRole::Assistant, content: serde_json::Value::Array(assistant) });
    request.messages.push(LlmMessage {
        role: LlmRole::User,
        content: json!([{
            "type": "tool_result",
            "tool_use_id": tool_use_id,
            "content": r#"{"city":"Paris","temperature_c":18,"conditions":"sunny"}"#,
            "cache_control": {"type": "ephemeral"},
        }]),
    });
    let second = stream_request(&client, request).await;

    let text: String = second
        .iter()
        .filter_map(|e| match e {
            LlmEvent::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let stop_reason = second.iter().find_map(|e| match e {
        LlmEvent::MessageDelta { stop_reason: Some(reason), .. } => Some(reason.clone()),
        _ => None,
    });
    eprintln!("second call: stop reason {stop_reason:?}, text {text:?}");
    assert!(!text.trim().is_empty(), "no text answer after the tool result");
    assert!(stop_reason.is_some(), "second call reached no stop reason");
}
