use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Genre {
    pub id: i32,
    pub name: String
}