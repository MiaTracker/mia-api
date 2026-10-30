use serde::Deserialize;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
pub struct ConversationParams {
    pub conversation_id: i32
}

#[derive(Deserialize, IntoParams)]
pub struct ExchangeStreamParams {
    pub conversation_id: i32,
    pub exchange_id: i32
}
