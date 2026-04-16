use serde::Serialize;

#[derive(Serialize)]
pub struct RefreshResult {
    pub updates: usize,
    pub errors: usize,
}