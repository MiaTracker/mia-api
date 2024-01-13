use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Languages {
    pub english_name: String,
    pub iso_639_1: String,
    pub name: String
}