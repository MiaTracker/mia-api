use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::{MaybeRouteType, RouteType};
use views::images::ImagesUpdate;
use views::media::{MediaCreateParams, MediaDeletePathParams, SearchQuery, MediaSourceCreate, MediaSourceDelete, MediaType, SearchParams, PageReq};
use views::movies::{MovieDetails, MovieMetadata};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::movies::create(params.tmdb_id, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

pub async fn create_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceCreate>) -> impl IntoResponse {
    let result = services::media::create_w_source(json, MediaType::Movie, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::movies::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::Movies.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn genres(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::genres::index(Some(MediaType::Movie), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::details(movie_id, &user, &state.conn).await;
    result.map_to_response(|movie: &Option<MovieDetails>| {
        match movie {
            None => { (StatusCode::NOT_FOUND, None) }
            Some(details) => { (StatusCode::OK, Some(details)) }
        }
    })
}

pub async fn metadata(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::metadata(movie_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn on_watchlist(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::on_watchlist(movie_id, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>,
                    Json(metadata): Json<MovieMetadata>) -> impl IntoResponse {
    let result = services::movies::update(movie_id, metadata, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn images(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::images(movie_id, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn update_images(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Path(movie_id): Path<i32>, Json(json): Json<ImagesUpdate>) -> impl IntoResponse {
    let result = services::media::update_images(movie_id, MediaType::Movie, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(path): Path<MediaDeletePathParams>) -> impl IntoResponse {
    let result = services::media::delete(path.media_id, RouteType::Movies.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceDelete>) -> impl IntoResponse {
    let result = services::media::delete_w_source(json.tmdb_id, json.source, MediaType::Movie, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}