use axum::http::StatusCode;
use axum::Router;
use axum::routing::{delete, get, post};
use crate::endpoints::{configuration, genres, logs, masterdata, movies, series, tags, titles, users};
use crate::infrastructure::AppState;

pub fn build() -> Router<AppState>
{
    Router::new()
        .route("/configuration/images", get(configuration::images))
        .route("/movies", post(movies::create))
        .route("/movies", get(movies::index))
        .route("/movies/:movie_id", get(movies::details))
        .route("/movies/:movie_id", delete(movies::delete))
        .route("/movies/:movie_id/metadata", get(movies::metadata))
        .route("/movies/:media_id/logs", post(logs::create))
        .route("/movies/:media_id/logs/:log_id", delete(logs::delete))
        .route("/series", post(series::create))
        .route("/series", get(series::index))
        .route("/series/:series_id", get(series::details))
        .route("/series/:series_id", delete(series::delete))
        .route("/series/:series_id/metadata", get(series::metadata))
        .route("/:route_type/:media_id/tags", post(tags::create))
        .route("/:route_type/:media_id/tags/:tag_id", delete(tags::delete))
        .route("/:route_type/:media_id/genres", post(genres::create))
        .route("/:route_type/:media_id/genres/:genre_id", delete(genres::delete))
        .route("/:route_type/:media_id/titles", post(titles::create))
        .route("/:route_type/:media_id/titles/:title_id/primary", post(titles::set_primary))
        .route("/:route_type/:media_id/titles/:title_id", delete(titles::delete))
}

pub fn build_anonymous() -> Router<AppState> {
    Router::new()
        .route("/ping", get(StatusCode::OK))
        .route("/users/login", post(users::login))
}

pub fn build_admin() -> Router<AppState> {
    Router::new()
        .route("/users/register", post(users::register))
        .route("/masterdata/refresh", post(masterdata::refresh))
}