use tower_http::cors::{CorsLayer};

pub fn build() -> CorsLayer {
    CorsLayer::very_permissive()
}