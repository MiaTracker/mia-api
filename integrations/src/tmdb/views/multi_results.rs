use serde::Deserialize;

#[derive(Deserialize)]
pub struct MultiResults {
    pub page: i32,
    pub results: Vec<MultiResult>,
    pub total_pages: i32,
    pub total_results: i32
}

#[derive(Deserialize)]
#[serde(tag = "media_type")]
pub enum MultiResult {
    #[serde(rename = "movie")]
    Movie(MultiMovieResult),
    #[serde(rename = "tv")]
    Tv(MultiTvResult),
    #[serde(rename = "person")]
    Person(MultiPersonResult),
    #[serde(rename = "collection")]
    Collection(MultiCollectionResult)
}

#[derive(Deserialize)]
pub struct MultiMovieResult {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub id: i32,
    pub title: Option<String>,
    pub original_language: Option<String>,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub genre_ids: Option<Vec<i32>>,
    pub popularity: Option<f32>,
    pub release_date: Option<String>,
    pub video: Option<bool>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>
}

#[derive(Deserialize)]
pub struct MultiTvResult {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub id: i32,
    pub name: Option<String>,
    pub original_language: Option<String>,
    pub original_name: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub genre_ids: Option<Vec<i32>>,
    pub popularity: Option<f32>,
    pub first_air_date: Option<String>,
    pub vote_average: Option<f32>,
    pub vote_count: Option<i32>,
    pub origin_country: Option<Vec<String>>
}

#[derive(Deserialize)]
pub struct MultiPersonResult {
    pub adult: Option<bool>,
    pub id: i32,
    pub name: Option<String>,
    pub original_name: Option<String>,
    pub popularity: Option<f32>,
    pub gender: Option<i32>,
    pub known_for_department: Option<String>,
    pub profile_path: Option<String>,
    pub known_for: Option<Vec<KnownForResult>>,
}

#[derive(Deserialize)]
#[serde(tag = "media_type")]
pub enum KnownForResult {
    #[serde(rename = "movie")]
    Movie(MultiMovieResult),
    #[serde(rename = "tv")]
    Tv(MultiTvResult)
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

#[derive(Deserialize)]
pub struct MultiCollectionResult {
    pub adult: Option<bool>,
    pub backdrop_path: Option<String>,
    pub id: i32,
    pub title: Option<String>,
    pub original_language: Option<String>,
    pub original_title: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
}