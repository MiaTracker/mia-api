use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::media::MediaCreateParams;
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::series::create(params.tmdb_id, &user, &state.conn).await;
    match result {
        Ok(created) => {
            if created {
                StatusCode::CREATED.into_response()
            } else {
                StatusCode::OK.into_response()
            }
        }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::series::index(&user, &state.conn).await;
    match result {
        Ok(series) => { Json(series).into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::details(&user, series_id, &state.conn).await;
    match result {
        Ok(movie) => { match movie {
            None => { StatusCode::NOT_FOUND.into_response() }
            Some(some) => { Json(some).into_response() }
        }}
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}