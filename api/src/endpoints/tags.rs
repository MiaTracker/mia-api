use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::tags::{TagCreate, TagCreateParams, TagDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(params): Path<TagCreateParams>, Json(tag): Json<TagCreate>) -> impl IntoResponse {
    let result = services::tags::create(params.media_id, &tag, params.route_type.into(), &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(params): Path<TagDeleteParams>) -> impl IntoResponse {
    let result = services::tags::delete(params.media_id, params.tag_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}