use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Default)]
pub struct MovieTitles {
    pub id: i32,
    pub titles: Vec<MovieTitle>
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct MovieTitle {
    pub iso_3166_1: String,
    pub title: String,
    pub r#type: String
}