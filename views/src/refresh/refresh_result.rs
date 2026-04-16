use serde::Serialize;

#[derive(Serialize)]
pub struct RefreshResult {
    pub updated: usize,
    pub errors: usize,
}