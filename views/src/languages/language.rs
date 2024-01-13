use serde::Serialize;

#[derive(Serialize)]
pub struct Language {
    pub iso_639_1: String,
    pub english_name: String,
    pub name: String
}