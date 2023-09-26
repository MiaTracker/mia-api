use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct UserTokenClaims {
    pub sub: String,
    pub iat: usize,
    pub exp: usize
}