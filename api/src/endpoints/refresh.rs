use crate::infrastructure::{AppState, IntoApiResponse};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;


#[utoipa::path(
    post,
    operation_id = "refresh::refresh",
    path = "/refresh",
    responses(
        (status = 200, description = "Refresh completed"),
        (status = 401, description = "Authorization token was not provided, was invalid or the user is not an admin"),
        (status = 500, description = "An internal error occurred while processing the request")
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn refresh(state: State<AppState>) -> impl IntoResponse {
    let result = services::refresh::refresh(&state.conn, state.tmdb.as_ref(), state.image_store.as_ref(), state.jobs.as_ref(), state.clock.as_ref(), state.random.as_ref()).await;
    result.to_response(StatusCode::OK)
}
