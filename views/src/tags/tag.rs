use serde::Serialize;
use entities::tags::Model;

#[derive(Serialize)]
pub struct Tag {
    pub id: i32,
    pub name: String
}

impl From<&Model> for Tag {
    fn from(value: &Model) -> Self {
        Self {
            id: value.id,
            name: value.name.clone(),
        }
    }
}