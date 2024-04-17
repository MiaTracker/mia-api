use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

#[derive(Deserialize, IntoParams)]
pub struct UserDeleteParams {
    #[param(value_type = String)]
    pub uuid: Uuid
}