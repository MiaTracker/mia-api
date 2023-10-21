use axum::http::StatusCode;
use axum::Router;
use axum::routing::{get, post};
use crate::endpoints::{masterdata, media, users};
use crate::infrastructure::AppState;

pub fn build() -> Router<AppState>
{
    Router::new()
        .route("/media", post(media::create))
}

pub fn build_anonymous() -> Router<AppState> {
    Router::new()
        .route("/ping", get(StatusCode::OK))
        .route("/masterdata/refresh", post(masterdata::refresh))
        .route("/users/register", post(users::register))
        .route("/users/login", post(users::login))
}