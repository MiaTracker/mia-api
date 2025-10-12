use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct PingResponse {
    pub status: PingStatus
}

#[derive(Serialize, ToSchema)]
pub enum PingStatus {
    #[serde(rename = "up")]
    Up
}