use serde::Deserialize;

#[derive(Deserialize)]
pub struct WatchlistSearchQueryParams {
    pub query: String
}