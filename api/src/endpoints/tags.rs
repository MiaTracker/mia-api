use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::media::MediaType;
use views::tags::{TagCreate, TagDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create_movie_tag(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(media_id): Path<i32>, Json(tag): Json<TagCreate>) -> impl IntoResponse {
    create(state, user, media_id, tag, MediaType::Movie).await
}

pub async fn create_series_tag(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(media_id): Path<i32>, Json(tag): Json<TagCreate>) -> impl IntoResponse {
    create(state, user, media_id, tag, MediaType::Series).await
}

pub async fn delete_movie_tag(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(params): Path<TagDeleteParams>) -> impl IntoResponse {
    delete(state, user, params, MediaType::Movie).await
}

pub async fn delete_series_tag(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(params): Path<TagDeleteParams>) -> impl IntoResponse {
    delete(state, user, params, MediaType::Series).await
}

async fn create(state: State<AppState>, user: CurrentUser,
                    media_id: i32, tag: TagCreate, media_type: MediaType) -> impl IntoResponse {
    let result = services::tags::create(media_id, &tag, media_type, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::CREATED.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

async fn delete(state: State<AppState>, user: CurrentUser,
                    params: TagDeleteParams, media_type: MediaType) -> impl IntoResponse {
    let result = services::tags::delete(params.media_id, params.tag_id, media_type, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}