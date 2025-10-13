use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum::response::IntoResponse;
use views::app_tokens::{AppTokenGenerate, AppTokenRevokeParams, AppToken, AppTokenIndex};
use views::users::CurrentUser;
use views::api::ApiErrView;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    operation_id = "app_tokens::generate",
    path = "/app_tokens",
    request_body = AppTokenGenerate,
    responses(
        (status = 201, description = "New api token was generated", body = AppToken),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn generate(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(req): Json<AppTokenGenerate>) -> impl IntoResponse {
    let result = services::app_tokens::generate(req.name, &state.jwt_secret, &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

#[utoipa::path(
    get,
    operation_id = "app_tokens::index",
    path = "/app_tokens",
    responses(
        (status = 200, description = "All tokens of the user", body = [Vec<AppTokenIndex>]),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::app_tokens::index(&user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    operation_id = "app_tokens::revoke",
    path = "/app_tokens/{name}",
    params(AppTokenRevokeParams),
    responses(
        (status = 200, description = "Token revoked"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "Token with this name was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn revoke(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<AppTokenRevokeParams>) -> impl IntoResponse {
    let result = services::app_tokens::revoke(params.name, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}
