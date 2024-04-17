use serde::Deserialize;
use utoipa::ToSchema;

#[derive(Deserialize, ToSchema)]
pub struct UserRegistration {
    pub email: String,
    pub username: String,
    pub password: String,
    pub password_repeat: String
}

