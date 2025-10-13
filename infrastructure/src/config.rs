use std::fs::File;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use serde::Deserialize;
use crate::fail;

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

    set_config(config);
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
    pub media: MediaConfiguration
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
    pub authorization_token: String
}

#[derive(Deserialize)]
pub struct LoggingConfiguration {
    pub level: LogLevel
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