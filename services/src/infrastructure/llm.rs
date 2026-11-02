use std::sync::Arc;
use infrastructure::{DataPolicy, OpenRouterConfiguration};
use integrations::openrouter;
use integrations::openrouter::{Llm, OpenRouterClient};

/// The OpenRouter client the configuration describes.
pub fn openrouter_llm(config: &OpenRouterConfiguration) -> Arc<dyn Llm> {
    Arc::new(OpenRouterClient::new(
        config.api_key.clone(),
        config.model.clone(),
        config.max_tokens,
        config.reasoning.as_ref().map(|reasoning| reasoning.budget_tokens),
        data_policy(config.data_policy),
    ))
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
    fn data_policy_maps_each_configured_policy_to_its_client_policy() {
        assert_eq!(data_policy(DataPolicy::Allow), openrouter::DataPolicy::Allow);
        assert_eq!(data_policy(DataPolicy::Deny), openrouter::DataPolicy::Deny);
        assert_eq!(data_policy(DataPolicy::Zdr), openrouter::DataPolicy::Zdr);
    }
}
