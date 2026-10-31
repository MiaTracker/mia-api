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
    pub reasoning: Option<ReasoningConfiguration>
}

impl OpenRouterConfiguration {
    /// Checks the values serde cannot: the reasoning budget must leave room for the answer.
    pub fn validate(&self) -> Result<(), String> {
        match &self.reasoning {
            Some(reasoning) if reasoning.budget_tokens >= self.max_tokens => Err(format!(
                "openrouter.reasoning.budget_tokens ({}) must be below openrouter.max_tokens ({})",
                reasoning.budget_tokens, self.max_tokens
            )),
            _ => Ok(())
        }
    }
}

#[derive(Deserialize)]
pub struct ReasoningConfiguration {
    pub budget_tokens: u32
}

fn default_max_tokens() -> u32 {
    16384
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

    fn openrouter(max_tokens: u32, reasoning: Option<ReasoningConfiguration>) -> OpenRouterConfiguration {
        OpenRouterConfiguration { api_key: "k".into(), model: "m".into(), max_tokens, reasoning }
    }

    #[test]
    fn validate_fails_when_budget_tokens_not_below_max_tokens() {
        let config = openrouter(4096, Some(ReasoningConfiguration { budget_tokens: 4096 }));

        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_succeeds_when_budget_below_max_tokens() {
        let config = openrouter(4096, Some(ReasoningConfiguration { budget_tokens: 2048 }));

        assert_eq!(config.validate(), Ok(()));
    }

    #[test]
    fn validate_succeeds_when_reasoning_absent() {
        assert_eq!(openrouter(4096, None).validate(), Ok(()));
    }

    #[test]
    fn openrouter_configuration_reads_budget_tokens_when_reasoning_present() {
        let config: OpenRouterConfiguration =
            serde_yaml::from_str("api_key: k\nmodel: m\nreasoning:\n  budget_tokens: 8000\n").unwrap();

        assert_eq!(config.reasoning.unwrap().budget_tokens, 8000);
        assert_eq!(config.max_tokens, 16384);
    }
}
