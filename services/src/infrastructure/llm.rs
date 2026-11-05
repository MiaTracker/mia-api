use std::sync::Arc;
use infrastructure::{DataPolicy, OpenRouterConfiguration, ReasoningEffort};
use integrations::openrouter;
use integrations::openrouter::{Llm, OpenRouterClient};

/// The OpenRouter client the configuration describes.
pub fn openrouter_llm(config: &OpenRouterConfiguration) -> Arc<dyn Llm> {
    Arc::new(OpenRouterClient::new(
        config.api_key.clone(),
        config.model.clone(),
        config.max_tokens,
        config.reasoning.as_ref().map(|reasoning| reasoning_effort(reasoning.effort)),
        data_policy(config.data_policy),
    ))
}

/// Most tokens a conversation title may take.
pub const TITLE_MAX_TOKENS: u32 = 64;

/// The OpenRouter client that titles conversations, or `None` when no `title_model` is configured.
pub fn openrouter_title_llm(config: &OpenRouterConfiguration) -> Option<Arc<dyn Llm>> {
    let title_model = config.title_model.clone()?;
    Some(Arc::new(OpenRouterClient::new(
        config.api_key.clone(),
        title_model,
        TITLE_MAX_TOKENS,
        None,
        data_policy(config.data_policy),
    )))
}

fn reasoning_effort(effort: ReasoningEffort) -> openrouter::ReasoningEffort {
    match effort {
        ReasoningEffort::Low => openrouter::ReasoningEffort::Low,
        ReasoningEffort::Medium => openrouter::ReasoningEffort::Medium,
        ReasoningEffort::High => openrouter::ReasoningEffort::High,
        ReasoningEffort::Xhigh => openrouter::ReasoningEffort::Xhigh,
        ReasoningEffort::Max => openrouter::ReasoningEffort::Max,
    }
}

fn data_policy(policy: DataPolicy) -> openrouter::DataPolicy {
    match policy {
        DataPolicy::Allow => openrouter::DataPolicy::Allow,
        DataPolicy::Deny => openrouter::DataPolicy::Deny,
        DataPolicy::Zdr => openrouter::DataPolicy::Zdr,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reasoning_effort_maps_each_configured_effort_to_its_client_effort() {
        assert_eq!(reasoning_effort(ReasoningEffort::Low), openrouter::ReasoningEffort::Low);
        assert_eq!(reasoning_effort(ReasoningEffort::Medium), openrouter::ReasoningEffort::Medium);
        assert_eq!(reasoning_effort(ReasoningEffort::High), openrouter::ReasoningEffort::High);
        assert_eq!(reasoning_effort(ReasoningEffort::Xhigh), openrouter::ReasoningEffort::Xhigh);
        assert_eq!(reasoning_effort(ReasoningEffort::Max), openrouter::ReasoningEffort::Max);
    }

    #[test]
    fn data_policy_maps_each_configured_policy_to_its_client_policy() {
        assert_eq!(data_policy(DataPolicy::Allow), openrouter::DataPolicy::Allow);
        assert_eq!(data_policy(DataPolicy::Deny), openrouter::DataPolicy::Deny);
        assert_eq!(data_policy(DataPolicy::Zdr), openrouter::DataPolicy::Zdr);
    }

    fn config_with_title_model(title_model: Option<&str>) -> OpenRouterConfiguration {
        OpenRouterConfiguration {
            api_key: "k".into(),
            model: "m".into(),
            max_tokens: 16384,
            reasoning: None,
            data_policy: DataPolicy::Deny,
            title_model: title_model.map(String::from),
        }
    }

    #[test]
    fn openrouter_title_llm_returns_none_when_title_model_absent() {
        assert!(openrouter_title_llm(&config_with_title_model(None)).is_none());
    }

    #[test]
    fn openrouter_title_llm_uses_title_model_when_present() {
        let llm = openrouter_title_llm(&config_with_title_model(Some("t/m"))).unwrap();

        assert_eq!(llm.model(), "t/m");
    }
}
