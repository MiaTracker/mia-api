use serde::Serialize;

#[derive(Serialize)]
pub struct Log {
    pub id: i32,
    pub date: chrono::NaiveDate,
    pub source: String,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>
}