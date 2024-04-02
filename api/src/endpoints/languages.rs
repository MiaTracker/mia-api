use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn index(state: State<AppState>) -> impl IntoResponse {
    let result = services::languages::index(&state.conn).await;
    result.to_response(StatusCode::OK)
}