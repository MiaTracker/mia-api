use serde::Deserialize;

#[derive(Deserialize)]
pub struct WatchlistParams {
    pub media_id: i32
}