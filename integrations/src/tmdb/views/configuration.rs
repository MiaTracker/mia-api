use serde::{Deserialize, Serialize};

#[derive(Deserialize, Clone, Debug)]
pub struct Configuration {
    pub images: ImagesConfiguration,
    pub change_keys: Vec<String>
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ImagesConfiguration {
    pub base_url: String,
    pub secure_base_url: String,
    pub backdrop_sizes: Vec<String>,
    pub logo_sizes: Vec<String>,
    pub poster_sizes: Vec<String>,
    pub profile_sizes: Vec<String>,
    pub still_sizes: Vec<String>
}