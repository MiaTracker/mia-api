use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum::response::IntoResponse;
use views::users::{CurrentUser, UserDeleteParams, UserLogin, UserRegistration};
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn register(state: State<AppState>, Json(user): Json<UserRegistration>) -> impl IntoResponse {
    let result = services::users::register(&user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

pub async fn login(state: State<AppState>, Json(user): Json<UserLogin>) -> impl IntoResponse {
    let result = services::users::login(&user, &state.jwt_secret, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn profile(Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::users::profile(&user);
    (StatusCode::OK, Json(result)).into_response()
}

pub async fn index(state: State<AppState>) -> impl IntoResponse {
    let result = services::users::index(&state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Path(params): Path<UserDeleteParams>) -> impl IntoResponse {
    let result = services::users::delete(params.uuid, &state.conn).await;
    result.to_response(StatusCode::OK)
}