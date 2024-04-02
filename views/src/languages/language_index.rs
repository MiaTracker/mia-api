use serde::Serialize;

#[derive(Serialize)]
pub struct LanguageIndex {
    pub iso_639_1: String,
    pub english_name: String
}