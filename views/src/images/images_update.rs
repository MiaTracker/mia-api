use serde::Deserialize;

#[derive(Deserialize)]
pub struct ImagesUpdate {
    pub backdrop_path: Option<String>,
    pub poster_path: Option<String>
}