use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    get,
    path = "/languages",
    responses(
        (status = 200, description = "All valid languages", body = [LanguageIndex]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>) -> impl IntoResponse {
    let result = services::languages::index(&state.conn).await;
    result.to_response(StatusCode::OK)
}