use serde::Deserialize;

#[derive(Deserialize)]
pub struct UserRegistration {
    pub email: String,
    pub username: String,
    pub password: String,
    pub password_repeat: String
}

