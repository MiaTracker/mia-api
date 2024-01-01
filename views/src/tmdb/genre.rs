use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct Genre {
    pub id: i32,
    pub name: String
}

#[derive(Deserialize, Clone)]
pub struct GenreList {
    pub genres: Vec<Genre>
}