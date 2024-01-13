use serde::Serialize;

#[derive(Serialize)]
pub struct AlternativeTitle {
    pub id: i32,
    pub title: String
}