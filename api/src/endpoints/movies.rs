use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::media::MediaCreateParams;
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::movies::create(params.tmdb_id, &user, &state.conn).await;
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
    let result = services::movies::index(&user, &state.conn).await;
    match result {
        Ok(movies) => { Json(movies).into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::details(movie_id, &user, &state.conn).await;
    match result {
        Ok(movie) => { match movie {
            None => { StatusCode::NOT_FOUND.into_response() }
            Some(some) => { Json(some).into_response() }
        }}
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn metadata(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::metadata(movie_id, &user, &state.conn).await;
    match result {
        Ok(movie) => { Json(movie).into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::delete(movie_id, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}