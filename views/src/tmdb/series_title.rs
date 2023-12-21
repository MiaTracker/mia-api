use serde::Deserialize;

#[derive(Deserialize)]
pub struct SeriesTitles {
    pub id: i32,
    pub results: Vec<SeriesTitle>
}

#[derive(Deserialize)]
pub struct SeriesTitle {
    pub iso_3166_1: String,
    pub title: String,
    pub r#type: String
}