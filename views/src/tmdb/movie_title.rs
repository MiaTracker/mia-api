use serde::Deserialize;

#[derive(Deserialize)]
pub struct MovieTitles {
    pub id: i32,
    pub titles: Vec<MovieTitle>
}

#[derive(Deserialize)]
pub struct MovieTitle {
    pub iso_3166_1: String,
    pub title: String,
    pub r#type: String
}