use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Collection {
    pub id: i32,
    pub name: String,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>
}