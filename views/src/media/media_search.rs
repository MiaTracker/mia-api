use serde::Deserialize;
#[derive(Deserialize)]
pub struct MediaSearchQueryParams {
    pub query: String,
}