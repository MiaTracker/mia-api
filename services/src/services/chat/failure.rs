//! How an LLM error becomes the failure key and messages stored on a failed Exchange.
//!
//! Classification looks at the HTTP status of a pre-stream error and at the OpenRouter error type
//! of a mid-stream error. It never inspects the message text.

use std::fmt::{self, Display, Formatter};

use integrations::openrouter::{LlmError, UNPARSEABLE_RESPONSE};
use serde_json::{json, Value};
use views::conversations::ExchangeError;

/// Why an Exchange failed. The variant name is the failure key shown to clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeFailure {
    ProviderRateLimited,
    ProviderUnavailable,
    ProviderOutOfCredits,
    ProviderAuthFailed,
    Refused,
    ProviderError,
    EmptyResponse,
    Interrupted,
    Internal,
}

impl Display for ExchangeFailure {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// The failure for an error of the model call: by HTTP status when the request was rejected, by
/// OpenRouter error type when the stream failed midway, as a provider error when a request or
/// frame could not be parsed (retrying cannot help), and as unavailable when neither is known
/// (the request never got an answer).
pub fn classify(error: &LlmError) -> ExchangeFailure {
    if let Some(status) = error.status {
        return match status {
            429 => ExchangeFailure::ProviderRateLimited,
            402 => ExchangeFailure::ProviderOutOfCredits,
            401 | 403 => ExchangeFailure::ProviderAuthFailed,
            408 | 500..=599 => ExchangeFailure::ProviderUnavailable,
            _ => ExchangeFailure::ProviderError,
        };
    }
    match error.error_type.as_deref() {
        None => ExchangeFailure::ProviderUnavailable,
        Some("rate_limit_exceeded") => ExchangeFailure::ProviderRateLimited,
        Some("provider_overloaded" | "provider_unavailable" | "server" | "timeout") => {
            ExchangeFailure::ProviderUnavailable
        }
        Some("payment_required") => ExchangeFailure::ProviderOutOfCredits,
        Some("authentication" | "permission_denied") => ExchangeFailure::ProviderAuthFailed,
        Some("refusal" | "content_policy_violation") => ExchangeFailure::Refused,
        Some(UNPARSEABLE_RESPONSE) => ExchangeFailure::ProviderError,
        Some(_) => ExchangeFailure::ProviderError,
    }
}

/// The text for `exchanges.error.debug_message`: the status or error type and the message.
pub fn debug_message(error: &LlmError) -> String {
    if let Some(status) = error.status {
        format!("{status}: {}", error.message)
    } else if let Some(error_type) = &error.error_type {
        format!("{error_type}: {}", error.message)
    } else {
        error.message.clone()
    }
}

/// The JSON for `llm_calls.error`. All three keys are always present; absent values are `null`.
pub fn llm_call_error(error: &LlmError) -> Value {
    json!({
        "status": error.status,
        "message": error.message,
        "response": error.response,
    })
}

/// The error stored on a failed Exchange, keyed by the failure's name.
pub fn exchange_error(failure: ExchangeFailure, debug_message: String) -> ExchangeError {
    ExchangeError { key: failure.to_string(), debug_message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{self, llm::{http_error, stream_error}};
    use integrations::openrouter::LlmError;
    use serde_json::json;

    #[test]
    fn classify_returns_rate_limited_when_status_429() {
        test_support::init();
        assert_eq!(classify(&http_error(429, "slow down")), ExchangeFailure::ProviderRateLimited);
    }

    #[test]
    fn classify_returns_provider_error_when_status_400() {
        test_support::init();
        assert_eq!(classify(&http_error(400, "bad request")), ExchangeFailure::ProviderError);
    }

    #[test]
    fn classify_returns_out_of_credits_when_status_402() {
        test_support::init();
        assert_eq!(classify(&http_error(402, "no credits")), ExchangeFailure::ProviderOutOfCredits);
    }

    #[test]
    fn classify_returns_auth_failed_when_status_401() {
        test_support::init();
        assert_eq!(classify(&http_error(401, "bad key")), ExchangeFailure::ProviderAuthFailed);
    }

    #[test]
    fn classify_returns_unavailable_when_status_503() {
        test_support::init();
        assert_eq!(classify(&http_error(503, "down")), ExchangeFailure::ProviderUnavailable);
    }

    #[test]
    fn classify_returns_unavailable_when_no_status_and_no_error_type() {
        test_support::init();
        let error = LlmError { status: None, error_type: None, message: "connection reset".into(), response: None };
        assert_eq!(classify(&error), ExchangeFailure::ProviderUnavailable);
    }

    #[test]
    fn classify_returns_refused_when_stream_error_is_content_policy_violation() {
        test_support::init();
        assert_eq!(classify(&stream_error("content_policy_violation", "no")), ExchangeFailure::Refused);
    }

    #[test]
    fn classify_returns_provider_error_when_stream_error_type_unknown() {
        test_support::init();
        assert_eq!(classify(&stream_error("something_new", "?")), ExchangeFailure::ProviderError);
    }

    #[test]
    fn classify_returns_provider_error_when_response_unparseable() {
        test_support::init();
        let error = LlmError {
            status: None,
            error_type: Some(UNPARSEABLE_RESPONSE.into()),
            message: "Serialization error: missing field `signature`".into(),
            response: None,
        };
        assert_eq!(classify(&error), ExchangeFailure::ProviderError);
    }

    #[test]
    fn debug_message_starts_with_status_when_status_present() {
        test_support::init();
        assert_eq!(debug_message(&http_error(400, "bad request")), "400: bad request");
    }

    #[test]
    fn llm_call_error_keeps_raw_response_when_present() {
        test_support::init();
        let error = LlmError {
            status: Some(400),
            error_type: None,
            message: "bad request".into(),
            response: Some(json!({"error": {"code": 400}})),
        };
        assert_eq!(
            llm_call_error(&error),
            json!({"status": 400, "message": "bad request", "response": {"error": {"code": 400}}})
        );
    }
}
