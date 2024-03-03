use axum::http::StatusCode;
use axum::Router;
use axum::routing::{delete, get, patch, post};
use crate::endpoints::{app_tokens, configuration, genres, logs, logset, masterdata, media, movies, series, sources, statistics, tags, titles, users, watchlist};
use crate::infrastructure::AppState;


pub fn build_bot() -> Router<AppState> {
    Router::new()
        .route("/movies/source_create", post(movies::create_w_source))
        .route("/movies/source_delete", post(movies::delete_w_source))
        .route("/series/source_create", post(series::create_w_source))
        .route("/series/source_delete", post(series::delete_w_source))
}

pub fn build() -> Router<AppState>
{
    Router::new()
        .route("/configuration/images", get(configuration::images))
        .route("/users/profile", get(users::profile))
        .route("/users/password", patch(users::change_password))
        .route("/app_tokens", get(app_tokens::index))
        .route("/app_tokens", post(app_tokens::generate))
        .route("/app_tokens/:name", delete(app_tokens::revoke))
        .route("/media", get(media::index))
        .route("/media/search", post(media::search))
        .route("/media/genres", get(media::genres))
        .route("/movies", post(movies::create))
        .route("/movies", get(movies::index))
        .route("/movies/search", post(movies::search))
        .route("/movies/genres", get(movies::genres))
        .route("/movies/:movie_id", get(movies::details))
        .route("/movies/:movie_id/metadata", get(movies::metadata))
        .route("/movies/:movie_id/metadata", patch(movies::update))
        .route("/movies/:movie_id/on_watchlist", get(movies::on_watchlist))
        .route("/movies/:movie_id", delete(movies::delete))
        .route("/series", post(series::create))
        .route("/series", get(series::index))
        .route("/series/search", post(series::search))
        .route("/series/genres", get(series::genres))
        .route("/series/:series_id", get(series::details))
        .route("/series/:series_id/metadata", get(series::metadata))
        .route("/series/:series_id/metadata", patch(series::update))
        .route("/series/:series_id/on_watchlist", get(series::on_watchlist))
        .route("/series/:series_id", delete(series::delete))
        .route("/:route_type/:media_id/tags", post(tags::create))
        .route("/:route_type/:media_id/tags/:tag_id", delete(tags::delete))
        .route("/:route_type/:media_id/genres", post(genres::create))
        .route("/:route_type/:media_id/genres/:genre_id", delete(genres::delete))
        .route("/:route_type/:media_id/titles", post(titles::create))
        .route("/:route_type/:media_id/titles/:title_id/primary", post(titles::set_primary))
        .route("/:route_type/:media_id/titles/:title_id", delete(titles::delete))
        .route("/:route_type/:media_id/sources", post(sources::create))
        .route("/:route_type/:media_id/sources", get(sources::index))
        .route("/:route_type/:media_id/sources/:source_id", get(sources::details))
        .route("/:route_type/:media_id/sources/:source_id", post(sources::update))
        .route("/:route_type/:media_id/sources/:source_id", delete(sources::delete))
        .route("/:route_type/:media_id/logs", post(logs::create))
        .route("/:route_type/:media_id/logs/:log_id", get(logs::details))
        .route("/:route_type/:media_id/logs/:log_id", post(logs::update))
        .route("/:route_type/:media_id/logs/:log_id", delete(logs::delete))
        .route("/watchlist", get(watchlist::index))
        .route("/watchlist/search", post(watchlist::search))
        .route("/watchlist/add", post(watchlist::add))
        .route("/watchlist/remove", post(watchlist::remove))
        .route("/logset", post(logset::create))
        .route("/statistics", get(statistics::stats))
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
        .route("/users/:uuid", delete(users::delete))
        .route("/masterdata/refresh", post(masterdata::refresh))
}