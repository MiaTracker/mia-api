use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<usize>,
    pub r#type: TokenType
}

#[derive(Eq, PartialEq, Clone, Serialize, Deserialize)]
pub enum TokenType {
    UserToken,
    AppToken
}