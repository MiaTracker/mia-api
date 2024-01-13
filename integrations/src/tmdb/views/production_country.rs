use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct ProductionCountry {
    pub iso_3166_1: String,
    pub name: String
}
