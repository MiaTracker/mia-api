use serde::Serialize;
use entities::languages::Model;

#[derive(Serialize)]
pub struct Language {
    pub iso_639_1: String,
    pub english_name: String,
    pub name: String
}

impl From<Model> for Language {
    fn from(value: Model) -> Self {
        Self {
            iso_639_1: value.iso6391,
            english_name: value.english_name,
            name: value.name,
        }
    }
}