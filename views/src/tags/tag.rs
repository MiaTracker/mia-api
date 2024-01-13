use serde::Serialize;

#[derive(Serialize)]
pub struct Tag {
    pub id: i32,
    pub name: String
}