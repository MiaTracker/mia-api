use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct AlternativeTitle {
    pub id: i32,
    pub title: String
}