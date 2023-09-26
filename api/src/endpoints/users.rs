use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use crate::infrastructure::{ApiErr, AppState};
use views::users::{UserLogin, UserRegistration};

pub async fn register(state: State<AppState>, Json(user): Json<UserRegistration>) -> impl IntoResponse {
    let result = services::users::register(&user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::CREATED.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn login(state: State<AppState>, Json(user): Json<UserLogin>) -> impl IntoResponse {
    let result = services::users::login(&user, &state.jwt_secret, &state.conn).await;
    match result {
        Ok(token) => Json(token).into_response(),
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}