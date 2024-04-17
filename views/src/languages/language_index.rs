use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct LanguageIndex {
    pub iso_639_1: String,
    pub english_name: String
}