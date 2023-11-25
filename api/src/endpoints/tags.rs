use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::tags::{TagCreate, TagDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(media_id): Path<i32>, Json(tag): Json<TagCreate>) -> impl IntoResponse {
    let result = services::tags::create(media_id, &tag, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::CREATED.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<TagDeleteParams>) -> impl IntoResponse {
    let result = services::tags::delete(params.media_id, params.tag_id, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}