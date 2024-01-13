use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use axum::response::Response;
use views::users::{UserLogin, UserRegistration};
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn register(state: State<AppState>, Json(user): Json<UserRegistration>) -> Response {
    let result = services::users::register(&user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

pub async fn login(state: State<AppState>, Json(user): Json<UserLogin>) -> Response {
    let result = services::users::login(&user, &state.jwt_secret, &state.conn).await;
    result.to_response(StatusCode::OK)
}