use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Languages {
    pub english_name: String,
    pub iso_639_1: String,
    pub name: String
}