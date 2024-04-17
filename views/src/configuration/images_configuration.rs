use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Deserialize, Serialize, ToSchema)]
pub struct ImagesConfiguration {
    pub base_url: String,
    pub secure_base_url: String,
    pub backdrop_sizes: Vec<String>,
    pub logo_sizes: Vec<String>,
    pub poster_sizes: Vec<String>,
    pub profile_sizes: Vec<String>,
    pub still_sizes: Vec<String>
}