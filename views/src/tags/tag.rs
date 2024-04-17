use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Tag {
    pub id: i32,
    pub name: String
}