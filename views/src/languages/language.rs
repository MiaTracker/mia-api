use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Language {
    pub iso_639_1: String,
    pub english_name: String,
    pub name: String
}