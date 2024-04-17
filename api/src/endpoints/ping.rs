use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;

use crate::infrastructure::AppState;

#[utoipa::path(
    get,
    path = "/ping",
    responses(
        (status = 200, description = "Service reachable"),
    )
)]
pub async fn ping(_state: State<AppState>) -> impl IntoResponse {
    StatusCode::OK.into_response()
}