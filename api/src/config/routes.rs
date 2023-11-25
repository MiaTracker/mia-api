use axum::http::StatusCode;
use axum::Router;
use axum::routing::{delete, get, post};
use crate::endpoints::{configuration, logs, masterdata, media, movies, tags, users};
use crate::infrastructure::AppState;

pub fn build() -> Router<AppState>
{
    Router::new()
        .route("/media", post(media::create))
        .route("/configuration/images", get(configuration::images))
        .route("/movies", get(movies::index))
        .route("/movies/:media_id", get(movies::details))
        .route("/movies/:media_id/logs", post(logs::create))
        .route("/movies/:media_id/logs/:log_id", delete(logs::delete))
        .route("/movies/:media_id/tags", post(tags::create))
        .route("/movies/:media_id/tags/:tag_id", delete(tags::delete))
}

pub fn build_anonymous() -> Router<AppState> {
    Router::new()
        .route("/ping", get(StatusCode::OK))
        .route("/masterdata/refresh", post(masterdata::refresh))
        .route("/users/register", post(users::register))
        .route("/users/login", post(users::login))
}