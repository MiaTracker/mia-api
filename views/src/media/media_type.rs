use serde::Deserialize;

#[derive(Deserialize)]
pub enum MediaType {
    Movie,
    Series
}