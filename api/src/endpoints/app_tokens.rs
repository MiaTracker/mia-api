use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum::response::IntoResponse;
use views::app_tokens::{AppTokenGenerate, AppTokenRevokeParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn generate(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(req): Json<AppTokenGenerate>) -> impl IntoResponse {
    let result = services::app_tokens::generate(req.name, &state.jwt_secret, &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::app_tokens::index(&user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn revoke(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<AppTokenRevokeParams>) -> impl IntoResponse {
    let result = services::app_tokens::revoke(params.name, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}
