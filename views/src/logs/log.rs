use serde::Serialize;
use entities::logs::Model;

#[derive(Serialize)]
pub struct Log {
    pub id: i32,
    pub date: chrono::NaiveDate,
    pub rating: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>
}

impl From<&Model> for Log {
    fn from(value: &Model) -> Self {
        Self {
            id: value.id,
            date: value.date,
            rating: value.rating,
            completed: value.completed,
            comment: value.comment.clone(),
        }
    }
}