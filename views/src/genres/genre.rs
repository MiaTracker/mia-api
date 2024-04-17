use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Genre {
    pub id: i32,
    pub name: String
}