use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct UserSummary {
    pub user_id: i32,
    pub user_uuid: Uuid,
    pub n: u32,
    pub k: u32,
    pub skip_reason: Option<String>,
    pub avg_before: Option<f32>,
    pub avg_after: Option<f32>,
    pub max_delta: Option<f32>,
}
