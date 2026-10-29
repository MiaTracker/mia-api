use serde::Deserialize;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct ProductionCompany {
    pub id: i32,
    pub logo_path: Option<String>,
    pub name: String,
    pub origin_country: Option<String>
}