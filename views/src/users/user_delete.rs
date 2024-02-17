use serde::Deserialize;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct UserDeleteParams {
    pub uuid: Uuid
}