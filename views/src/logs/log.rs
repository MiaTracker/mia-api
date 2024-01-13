use serde::Serialize;

#[derive(Serialize)]
pub struct Log {
    pub id: i32,
    pub date: chrono::NaiveDate,
    pub rating: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>
}