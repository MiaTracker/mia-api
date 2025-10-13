use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::ApiErrView;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    operation_id = "masterdata::refresh",
    path = "/masterdata/refresh",
    responses(
        (status = 200, description = "Masterdata refreshed"),
        (status = 401, description = "Authorization token was not provided, was invalid or the user is not an admin", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn refresh(state: State<AppState>) -> impl IntoResponse {
    let result = services::masterdata::refresh(&state.conn).await;
    result.to_response(StatusCode::OK)
}