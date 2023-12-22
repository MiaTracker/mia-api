use axum::http::StatusCode;
use axum::Router;
use axum::routing::{delete, get, post};
use crate::endpoints::{configuration, logs, masterdata, movies, series, tags, users};
use crate::infrastructure::AppState;

pub fn build() -> Router<AppState>
{
    Router::new()
        .route("/configuration/images", get(configuration::images))
        .route("/movies", post(movies::create))
        .route("/movies", get(movies::index))
        .route("/movies/:media_id", get(movies::details))
        .route("/movies/:media_id/logs", post(logs::create))
        .route("/movies/:media_id/logs/:log_id", delete(logs::delete))
        .route("/movies/:media_id/tags", post(tags::create))
        .route("/movies/:media_id/tags/:tag_id", delete(tags::delete))
        .route("/series", post(series::create))
}

pub fn build_anonymous() -> Router<AppState> {
    Router::new()
        .route("/ping", get(StatusCode::OK))
        .route("/masterdata/refresh", post(masterdata::refresh))
        .route("/users/register", post(users::register))
        .route("/users/login", post(users::login))
}