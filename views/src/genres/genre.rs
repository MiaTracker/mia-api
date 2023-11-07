use serde::Serialize;
use entities::genres::Model;

#[derive(Serialize)]
pub struct Genre {
    id: i32,
    name: String
}

impl From<&Model> for Genre {
    fn from(value: &Model) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
        }
    }
}