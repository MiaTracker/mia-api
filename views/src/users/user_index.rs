use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
pub struct UserIndex {
    #[schema(value_type = String)]
    pub uuid: Uuid,
    pub username: String,
    pub email: String,
    pub admin: bool
}