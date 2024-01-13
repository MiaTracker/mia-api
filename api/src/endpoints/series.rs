use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::media::MediaCreateParams;
use views::series::{SeriesDetails, SeriesMetadata};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::series::create(params.tmdb_id, &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::series::index(&user, &state.conn).await;
    result.to_response(StatusCode::OK)

}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::details(series_id, &user, &state.conn).await;
    result.map_to_response(|series: &Option<SeriesDetails>| {
        match series {
            None => { (StatusCode::NOT_FOUND, None) }
            Some(details) => { (StatusCode::OK, Some(details)) }
        }
    })
}

pub async fn metadata(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::metadata(series_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>, Json(metadata): Json<SeriesMetadata>) -> impl IntoResponse {
    let result = services::series::update(series_id, metadata, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::delete(movie_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}