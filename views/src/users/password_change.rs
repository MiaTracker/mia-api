use serde::Deserialize;

#[derive(Deserialize)]
pub struct PasswordChange {
    pub old_password: String,
    pub new_password: String,
    pub password_repeat: String
}