use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::languages::LanguageIndex;
use views::api::ApiErrView;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    get,
    path = "/languages",
    responses(
        (status = 200, description = "All valid languages", body = [LanguageIndex]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>) -> impl IntoResponse {
    let result = services::languages::index(&state.conn).await;
    result.to_response(StatusCode::OK)
}