use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum_macros::debug_handler;
use views::sources::{SourceCreate, SourceCreateParams, SourceDeleteParams, SourceUpdate, SourceUpdateParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[debug_handler]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<SourceCreateParams>, Json(source): Json<SourceCreate>) -> impl IntoResponse {
    let result = services::sources::create(params.media_id, &source, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<SourceUpdateParams>, Json(source): Json<SourceUpdate>) -> impl IntoResponse {
    let result = services::sources::update(params.media_id, params.source_id, &source, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<SourceDeleteParams>) -> impl IntoResponse {
    let result = services::sources::delete(params.media_id, params.source_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}