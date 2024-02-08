use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum::response::Response;
use views::app_tokens::{AppTokenGenerate, AppTokenRevokeParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn generate(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(req): Json<AppTokenGenerate>) -> Response {
    let result = services::app_tokens::generate(req.name, &state.jwt_secret, &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

pub async fn revoke(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<AppTokenRevokeParams>) -> Response {
    let result = services::app_tokens::revoke(params.name, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}
