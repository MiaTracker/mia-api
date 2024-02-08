use serde::Serialize;

#[derive(Serialize)]
pub struct AppToken {
    pub token: String
}