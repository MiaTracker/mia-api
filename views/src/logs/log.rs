use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct Log {
    pub id: i32,
    #[schema(value_type = String, format = Date)]
    pub date: chrono::NaiveDate,
    pub source: String,
    pub stars: Option<f32>,
    pub completed: bool,
    pub comment: Option<String>
}