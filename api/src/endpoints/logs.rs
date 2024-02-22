use axum::Extension;
use axum::extract::{Path, Json, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::logs::{LogCreate, LogCreateParams, LogDeleteParams, LogDetailsParams, LogUpdate, LogUpdateParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogCreateParams>, Json(log): Json<LogCreate>) -> impl IntoResponse {
    let result = services::logs::create(params.media_id, &log, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<LogDetailsParams>) -> impl IntoResponse {
    let result = services::logs::details(params.media_id, params.route_type.into(), params.log_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogUpdateParams>, Json(log): Json<LogUpdate>) -> impl IntoResponse {
    let result = services::logs::update(params.media_id, params.log_id, &log, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogDeleteParams>) -> impl IntoResponse {
    let result = services::logs::delete(params.media_id, params.log_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}