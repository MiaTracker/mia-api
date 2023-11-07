use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::movies::index(&user, &state.conn).await;
    match result {
        Ok(movies) => { Json(movies).into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(movie_id): Path<i32>) -> impl IntoResponse {
    let result = services::movies::details(&user, movie_id, &state.conn).await;
    match result {
        Ok(movie) => { match movie {
            None => { StatusCode::NOT_FOUND.into_response() }
            Some(some) => { Json(some).into_response() }
        }}
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}