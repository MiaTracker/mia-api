use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct ProductionCountry {
    pub iso_3166_1: String,
    pub name: String
}
