use crate::config::load_config;

mod config;
mod errors;

pub use config::*;

pub async fn initialize() {
    load_config().await;
}
