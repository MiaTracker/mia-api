use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn refresh(state: State<AppState>) -> impl IntoResponse {
    let result = services::masterdata::refresh(&state.conn).await;
    result.to_response(StatusCode::OK)
}