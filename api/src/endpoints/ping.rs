use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use axum::response::IntoResponse;
use views::ping::{PingResponse, PingStatus};

use crate::infrastructure::AppState;

#[utoipa::path(
    get,
    path = "/ping",
    responses(
        (status = 200, description = "Service reachable"),
    )
)]
pub async fn ping(_state: State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(
            PingResponse {
                status: PingStatus::Up
            }
        )
    ).into_response()
}