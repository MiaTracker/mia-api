use serde::Serialize;

#[derive(Serialize)]
pub struct Genre {
    pub id: i32,
    pub name: String
}