use chrono::NaiveDate;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct LogCreate {
    pub date: NaiveDate,
    pub rating: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>
}