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
}
