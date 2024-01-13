use axum::http::StatusCode;
use axum::response::IntoResponse;
use crate::infrastructure::IntoApiResponse;

pub async fn images() -> impl IntoResponse {
    let result = services::configuration::images().await;
    result.to_response(StatusCode::OK)
}