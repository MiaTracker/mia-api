use serde::Deserialize;

#[derive(Deserialize, Debug, Clone, Default)]
pub struct SeriesTitles {
    pub id: i32,
    pub results: Vec<SeriesTitle>
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct SeriesTitle {
    pub iso_3166_1: String,
    pub title: String,
    pub r#type: String
}