use serde::Deserialize;

#[derive(Deserialize)]
pub struct LogDeleteParams {
    pub media_id: i32,
    pub log_id: i32
}