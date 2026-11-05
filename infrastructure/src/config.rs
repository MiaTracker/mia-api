use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use serde::Deserialize;
use crate::fail;
use duration_str::deserialize_duration;

pub const CONFIG_FILE_PATH: &str = "config.yaml";

static CONFIG: OnceLock<Arc<Configuration>> = OnceLock::new();

pub fn config() -> Arc<Configuration> {
    match CONFIG.get() {
        Some(config) => { config.clone() }
        None => {
            fail!("Application config was accessed before being initialized! This is an internal developer error.");
        }
    }
}

pub(crate) async fn load_config() {
    let path = Path::new(CONFIG_FILE_PATH);

    let file = match File::open(&path) {
        Ok(file) => { file }
        Err(err) => {
            fail!("Failed to open {}: {}", path.display(), err);
        }
    };

    let config: Configuration = match serde_yaml::from_reader(file) {
        Ok(res) => { res }
        Err(err) => {
            fail!("Failed to deserialize config {}: {}", path.display(), err);
        }
    };

    if let Err(err) = config.assistant.validate() {
        fail!("Invalid config {}: {}", path.display(), err);
    }

    if let Err(err) = config.openrouter.validate() {
        fail!("Invalid config {}: {}", path.display(), err);
    }

    set_config(config);
}

/// Initializes the global config with the value produced by `f`, unless it has already been
/// initialized, in which case `f` is not called. Safe to call repeatedly (e.g. from tests).
pub fn init_config_once(f: impl FnOnce() -> Configuration) -> Arc<Configuration> {
    CONFIG.get_or_init(|| Arc::new(f())).clone()
}

fn set_config(config: Configuration) {
    match CONFIG.set(Arc::new(config)) {
        Ok(_) => { }
        Err(_) => {
            fail!("Tried to set application config more than once! This is an internal developer error.");
        }
    }
}

#[derive(Deserialize)]
pub struct Configuration {
    pub jwt: JwtConfiguration,
    pub db: DbConfiguration,
    pub tmdb: TMDBConfiguration,
    pub logging: LoggingConfiguration,
    pub media: MediaConfiguration,
    pub images: ImagesConfiguration,
    pub openrouter: OpenRouterConfiguration,
    #[serde(default)]
    pub assistant: AssistantConfiguration,
}

#[derive(Deserialize)]
pub struct JwtConfiguration {
    pub secret: String
}

#[derive(Deserialize)]
pub struct DbConfiguration {
    pub connection_url: String,
    pub schema: String
}

#[derive(Deserialize)]
pub struct TMDBConfiguration {
    pub authorization_token: String,
    #[serde(default, deserialize_with = "deserialize_duration")]
    pub refresh_interval: Option<Duration>
}

#[derive(Deserialize)]
pub struct LoggingConfiguration {
    pub level: LogLevel
}

#[derive(Deserialize)]
pub struct ImagesConfiguration {
    pub store_path: String
}

#[derive(Deserialize)]
pub struct OpenRouterConfiguration {
    pub api_key: String,
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Absent: reasoning is disabled.
    #[serde(default)]
    pub reasoning: Option<ReasoningConfiguration>,
    /// Which providers OpenRouter may route to. Absent: `deny`.
    #[serde(default)]
    pub data_policy: DataPolicy,
    /// Model that titles conversations, cheaper than `model`. Absent: conversations are not titled.
    #[serde(default)]
    pub title_model: Option<String>
}

impl OpenRouterConfiguration {
    /// Checks the values serde cannot: `title_model` must differ from `model`.
    pub fn validate(&self) -> Result<(), String> {
        match &self.title_model {
            Some(title_model) if *title_model == self.model => {
                Err("openrouter.title_model must differ from openrouter.model".into())
            }
            _ => Ok(())
        }
    }
}

/// How the upstream providers OpenRouter routes to may handle prompts.
#[derive(Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DataPolicy {
    /// Any provider, including those that may store or train on prompts.
    Allow,
    /// Only providers that do not collect prompts.
    #[default]
    Deny,
    /// Only endpoints with a zero-data-retention agreement.
    Zdr
}

#[derive(Deserialize)]
pub struct ReasoningConfiguration {
    pub effort: ReasoningEffort
}

/// How hard the model is asked to think.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
    Xhigh,
    Max
}

fn default_max_tokens() -> u32 {
    16384
}

/// Limits on what the chat assistant's tools may do.
#[derive(Deserialize)]
pub struct AssistantConfiguration {
    /// Fixed cap on the results of one `mia_search` call.
    #[serde(default = "default_mia_search_limit")]
    pub mia_search_limit: u32,
    /// Fixed cap on the ids of one `mia_details` call.
    #[serde(default = "default_mia_details_limit")]
    pub mia_details_limit: u32,
    /// How many rounds of tool calls one Exchange may run.
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: u32
}

impl Default for AssistantConfiguration {
    fn default() -> Self {
        AssistantConfiguration {
            mia_search_limit: default_mia_search_limit(),
            mia_details_limit: default_mia_details_limit(),
            max_tool_rounds: default_max_tool_rounds()
        }
    }
}

impl AssistantConfiguration {
    /// Checks the values serde cannot: every limit must be at least 1.
    pub fn validate(&self) -> Result<(), String> {
        let limits = [
            ("mia_search_limit", self.mia_search_limit),
            ("mia_details_limit", self.mia_details_limit),
            ("max_tool_rounds", self.max_tool_rounds)
        ];
        match limits.iter().find(|(_, value)| *value == 0) {
            Some((field, _)) => Err(format!("assistant.{field} must be at least 1")),
            None => Ok(())
        }
    }
}

fn default_mia_search_limit() -> u32 {
    30
}

fn default_mia_details_limit() -> u32 {
    60
}

fn default_max_tool_rounds() -> u32 {
    8
}

#[derive(Deserialize, Clone)]
pub enum LogLevel {
    #[serde(rename = "trace")]
    Trace,
    #[serde(rename = "debug")]
    Debug,
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "error")]
    Error
}

impl From<LogLevel> for tracing::level_filters::LevelFilter {
    fn from(value: LogLevel) -> Self {
        match value {
            LogLevel::Trace => { tracing::level_filters::LevelFilter::TRACE }
            LogLevel::Debug => { tracing::level_filters::LevelFilter::DEBUG }
            LogLevel::Info => { tracing::level_filters::LevelFilter::INFO }
            LogLevel::Warning => { tracing::level_filters::LevelFilter::WARN }
            LogLevel::Error => { tracing::level_filters::LevelFilter::ERROR }
        }
    }
}

#[derive(Deserialize)]
pub struct MediaConfiguration {
    pub unset_title: String
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openrouter_configuration_reads_each_effort_when_reasoning_present() {
        for (text, effort) in [
            ("low", ReasoningEffort::Low),
            ("medium", ReasoningEffort::Medium),
            ("high", ReasoningEffort::High),
            ("xhigh", ReasoningEffort::Xhigh),
            ("max", ReasoningEffort::Max),
        ] {
            let config: OpenRouterConfiguration =
                serde_yaml::from_str(&format!("api_key: k\nmodel: m\nreasoning:\n  effort: {text}\n")).unwrap();

            assert_eq!(config.reasoning.unwrap().effort, effort);
            assert_eq!(config.max_tokens, 16384);
        }
    }

    #[test]
    fn openrouter_configuration_rejects_unknown_effort_when_value_not_a_level() {
        let result = serde_yaml::from_str::<OpenRouterConfiguration>("api_key: k\nmodel: m\nreasoning:\n  effort: minimal\n");

        assert!(result.is_err());
    }

    #[test]
    fn openrouter_configuration_rejects_reasoning_when_effort_missing() {
        let result = serde_yaml::from_str::<OpenRouterConfiguration>("api_key: k\nmodel: m\nreasoning:\n  budget_tokens: 8000\n");

        assert!(result.is_err());
    }

    #[test]
    fn openrouter_configuration_disables_reasoning_when_absent() {
        let config: OpenRouterConfiguration = serde_yaml::from_str("api_key: k\nmodel: m\n").unwrap();

        assert!(config.reasoning.is_none());
    }

    #[test]
    fn openrouter_configuration_defaults_to_deny_when_data_policy_absent() {
        let config: OpenRouterConfiguration = serde_yaml::from_str("api_key: k\nmodel: m\n").unwrap();

        assert_eq!(config.data_policy, DataPolicy::Deny);
    }

    #[test]
    fn openrouter_configuration_reads_data_policy_when_present() {
        for (value, expected) in [("allow", DataPolicy::Allow), ("deny", DataPolicy::Deny), ("zdr", DataPolicy::Zdr)] {
            let config: OpenRouterConfiguration =
                serde_yaml::from_str(&format!("api_key: k\nmodel: m\ndata_policy: {value}\n")).unwrap();

            assert_eq!(config.data_policy, expected);
        }
    }

    #[test]
    fn openrouter_configuration_fails_when_data_policy_unknown() {
        let result = serde_yaml::from_str::<OpenRouterConfiguration>("api_key: k\nmodel: m\ndata_policy: maybe\n");

        assert!(result.is_err());
    }

    #[test]
    fn assistant_configuration_defaults_when_section_absent() {
        let config: Configuration = serde_yaml::from_str(concat!(
            "jwt:\n  secret: s\n",
            "db:\n  connection_url: u\n  schema: public\n",
            "tmdb:\n  authorization_token: t\n",
            "logging:\n  level: debug\n",
            "media:\n  unset_title: Unknown\n",
            "images:\n  store_path: p\n",
            "openrouter:\n  api_key: k\n  model: m\n"
        )).unwrap();

        assert_eq!(config.assistant.mia_search_limit, 30);
        assert_eq!(config.assistant.mia_details_limit, 60);
        assert_eq!(config.assistant.max_tool_rounds, 8);
    }

    #[test]
    fn assistant_configuration_keeps_defaults_when_field_absent() {
        let config: AssistantConfiguration = serde_yaml::from_str("mia_search_limit: 5\n").unwrap();

        assert_eq!(config.mia_search_limit, 5);
        assert_eq!(config.mia_details_limit, 60);
    }

    #[test]
    fn validate_fails_when_a_limit_is_zero() {
        for (field, config) in [
            ("mia_search_limit", AssistantConfiguration { mia_search_limit: 0, ..Default::default() }),
            ("mia_details_limit", AssistantConfiguration { mia_details_limit: 0, ..Default::default() }),
            ("max_tool_rounds", AssistantConfiguration { max_tool_rounds: 0, ..Default::default() }),
        ] {
            assert_eq!(config.validate(), Err(format!("assistant.{field} must be at least 1")));
        }
    }

    #[test]
    fn validate_succeeds_when_assistant_limits_are_defaults() {
        assert_eq!(AssistantConfiguration::default().validate(), Ok(()));
    }

    #[test]
    fn openrouter_configuration_has_no_title_model_when_absent() {
        let config: OpenRouterConfiguration = serde_yaml::from_str("api_key: k\nmodel: m\n").unwrap();

        assert_eq!(config.title_model, None);
    }

    #[test]
    fn openrouter_configuration_reads_title_model_when_present() {
        let config: OpenRouterConfiguration = serde_yaml::from_str("api_key: k\nmodel: m\ntitle_model: t/m\n").unwrap();

        assert_eq!(config.title_model, Some("t/m".to_string()));
    }

    #[test]
    fn validate_fails_when_title_model_equals_model() {
        let config: OpenRouterConfiguration = serde_yaml::from_str("api_key: k\nmodel: m\ntitle_model: m\n").unwrap();

        assert_eq!(config.validate(), Err("openrouter.title_model must differ from openrouter.model".into()));
    }

    #[test]
    fn validate_succeeds_when_title_model_absent_or_different() {
        for text in ["api_key: k\nmodel: m\n", "api_key: k\nmodel: m\ntitle_model: t/m\n"] {
            let config: OpenRouterConfiguration = serde_yaml::from_str(text).unwrap();

            assert_eq!(config.validate(), Ok(()));
        }
    }
}
