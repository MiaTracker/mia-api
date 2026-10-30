//! Contract test against the real OpenRouter API. It needs a key in `config.yaml`
//! (`openrouter.api_key`) and is ignored by default. It uses the free router, so it costs nothing.
//!
//! Run with `cargo test -p integrations --test openrouter_contract -- --ignored`.

use futures_util::StreamExt;
use integrations::openrouter::{Llm, LlmEvent, LlmMessage, LlmRequest, LlmRole, OpenRouterClient};
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

fn client() -> OpenRouterClient {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../config.yaml");
    let config: TestConfig = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_yaml::from_str(&text).ok())
        .expect("config.yaml with openrouter.api_key required for OpenRouter contract tests");
    OpenRouterClient::new(config.openrouter.api_key, "openrouter/free".to_string(), 512)
}

#[tokio::test]
#[ignore = "requires OpenRouter key"]
async fn stream_message_yields_text_and_stop_reason_when_prompt_is_short() {
    let request = LlmRequest {
        system: "Answer in one short sentence.".to_string(),
        messages: vec![LlmMessage {
            role: LlmRole::User,
            content: json!([{"type": "text", "text": "Say hello."}]),
        }],
    };

    let mut stream = client().stream_message(request).await.unwrap();
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream yielded an error"));
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, LlmEvent::TextDelta { text, .. } if !text.is_empty())));
    assert!(events
        .iter()
        .any(|e| matches!(e, LlmEvent::MessageDelta { stop_reason: Some(_), .. })));
}
