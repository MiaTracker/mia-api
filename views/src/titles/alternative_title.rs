use serde::Serialize;
use entities::titles::Model;

#[derive(Serialize)]
pub struct AlternativeTitle {
    pub id: i32,
    pub title: String
}

impl From<&Model> for AlternativeTitle {
    fn from(value: &Model) -> Self {
        Self {
            id: value.id,
            title: value.title.clone(),
        }
    }
}