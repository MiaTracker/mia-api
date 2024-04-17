use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct PasswordChange {
    pub old_password: String,
    pub new_password: String,
    pub password_repeat: String
}