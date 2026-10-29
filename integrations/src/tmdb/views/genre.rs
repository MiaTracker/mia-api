use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Genre {
    pub id: i32,
    pub name: String
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct GenreList {
    pub genres: Vec<Genre>
}