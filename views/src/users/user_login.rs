use serde::Deserialize;

#[derive(Deserialize)]
pub struct UserLogin {
    pub username: String,
    pub password: String
}