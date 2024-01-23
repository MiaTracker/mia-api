use serde::Deserialize;

#[derive(Deserialize)]
pub struct MultiResults {
    pub page: i32,
    pub results: Vec<MultiResult>,
    pub total_pages: i32,
    pub total_results: i32
}

#[derive(Deserialize)]
pub struct MultiResult {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub id: i32,
    pub title: Option<String>,
    pub original_language: Option<String>,
    pub original_title: Option<String>,
    pub poster_path: Option<String>,
    pub media_type: MultiResultMediaType,
    pub genre_ids: Option<Vec<i32>>,
    pub popularity: Option<f32>,
    pub release_date: Option<String>,
    pub video: Option<bool>
}

#[derive(Deserialize, Eq, PartialEq)]
pub enum MultiResultMediaType {
    #[serde(rename = "movie")]
    Movie,
    #[serde(rename = "tv")]
    Tv,
    #[serde(rename = "person")]
    Person,
}