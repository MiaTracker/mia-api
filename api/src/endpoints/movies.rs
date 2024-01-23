use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::{MaybeRouteType, RouteType};
use views::media::{MediaCreateParams, MediaDeletePathParams, MediaSearchQueryParams};
use views::movies::{MovieDetails, MovieMetadata};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::movies::create(params.tmdb_id, &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::movies::index(&user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaSearchQueryParams>) -> impl IntoResponse {
    let result = services::media::search(params.query, MaybeRouteType::Movies.into(), &user, &state.conn).await;
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

pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>,
                    Json(metadata): Json<MovieMetadata>) -> impl IntoResponse {
    let result = services::movies::update(movie_id, metadata, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(path): Path<MediaDeletePathParams>) -> impl IntoResponse {
    let result = services::media::delete(path.media_id, RouteType::Movies.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}