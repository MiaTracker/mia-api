use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct UserIndex {
    pub uuid: Uuid,
    pub username: String,
    pub email: String,
    pub admin: bool
}