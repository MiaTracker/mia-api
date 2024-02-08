use serde::Deserialize;

#[derive(Deserialize)]
pub struct AppTokenGenerate {
    pub name: String
}