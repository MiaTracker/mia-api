use axum::{Extension, Json};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::logset::LogsetCreate;
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/logset",
    request_body = LogsetCreate,
    responses(
        (status = 201, description = "Log created"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(logset): Json<LogsetCreate>) -> impl IntoResponse {
    let result = services::logset::create(&logset, &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}