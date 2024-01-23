use serde::Serialize;

#[derive(Serialize)]
pub struct UserProfile {
    pub username: String,
    pub email: String,
    pub admin: bool
}