use serde::Deserialize;

#[derive(Deserialize)]
pub struct AppTokenRevokeParams {
    pub name: String
}