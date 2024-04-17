use serde::Deserialize;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
pub struct AppTokenRevokeParams {
    pub name: String
}