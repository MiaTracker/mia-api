use axum::http::StatusCode;
use axum::Router;
use axum::routing::{delete, get, patch, post};
use crate::endpoints::{configuration, genres, logs, masterdata, media, movies, series, sources, tags, titles, users};
use crate::infrastructure::AppState;

pub fn build() -> Router<AppState>
{
    Router::new()
        .route("/configuration/images", get(configuration::images))
        .route("/users/profile", get(users::profile))
        .route("/media", get(media::index))
        .route("/media/search", get(media::search))
        .route("/movies", post(movies::create))
        .route("/movies", get(movies::index))
        .route("/movies/search", get(movies::search))
        .route("/movies/:movie_id", get(movies::details))
        .route("/movies/:movie_id", delete(movies::delete))
        .route("/movies/:movie_id/metadata", get(movies::metadata))
        .route("/movies/:movie_id/metadata", patch(movies::update))
        .route("/series", post(series::create))
        .route("/series", get(series::index))
        .route("/series/search", get(series::search))
        .route("/series/:series_id", get(series::details))
        .route("/series/:series_id", delete(series::delete))
        .route("/series/:series_id/metadata", get(series::metadata))
        .route("/series/:series_id/metadata", patch(series::update))
        .route("/:route_type/:media_id/tags", post(tags::create))
        .route("/:route_type/:media_id/tags/:tag_id", delete(tags::delete))
        .route("/:route_type/:media_id/genres", post(genres::create))
        .route("/:route_type/:media_id/genres/:genre_id", delete(genres::delete))
        .route("/:route_type/:media_id/titles", post(titles::create))
        .route("/:route_type/:media_id/titles/:title_id/primary", post(titles::set_primary))
        .route("/:route_type/:media_id/titles/:title_id", delete(titles::delete))
        .route("/:route_type/:media_id/sources", post(sources::create))
        .route("/:route_type/:media_id/sources/:source_id", post(sources::update))
        .route("/:route_type/:media_id/sources/:source_id", delete(sources::delete))
        .route("/:route_type/:media_id/logs", post(logs::create))
        .route("/:route_type/:media_id/logs/:log_id", post(logs::update))
        .route("/:route_type/:media_id/logs/:log_id", delete(logs::delete))
}

pub fn build_anonymous() -> Router<AppState> {
    Router::new()
        .route("/ping", get(StatusCode::OK))
        .route("/users/login", post(users::login))
}

pub fn build_admin() -> Router<AppState> {
    Router::new()
        .route("/users", get(users::index))
        .route("/users/register", post(users::register))
        .route("/masterdata/refresh", post(masterdata::refresh))
}