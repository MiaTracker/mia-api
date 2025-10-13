use axum::Extension;
use axum::extract::{Path, Json, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::logs::{LogCreate, LogCreateParams, LogDeleteParams, LogDetailsParams, LogUpdate, LogUpdateParams, Log};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};
use views::api::ApiErrView;

#[utoipa::path(
    post,
    path = "/{route_type}/{media_id}/logs",
    params(LogCreateParams),
    request_body = LogCreate,
    responses(
        (status = 201, description = "Log created"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogCreateParams>, Json(log): Json<LogCreate>) -> impl IntoResponse {
    let result = services::logs::create(params.media_id, &log, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

#[utoipa::path(
    get,
    path = "/{route_type}/{media_id}/logs/{log_id}",
    params(LogDetailsParams),
    responses(
        (status = 200, description = "Log details", body = Log),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The log was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<LogDetailsParams>) -> impl IntoResponse {
    let result = services::logs::details(params.media_id, params.route_type.into(), params.log_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/{route_type}/{media_id}/logs/{log_id}",
    params(LogUpdateParams),
    responses(
        (status = 200, description = "Log updated"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The log was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogUpdateParams>, Json(log): Json<LogUpdate>) -> impl IntoResponse {
    let result = services::logs::update(params.media_id, params.log_id, &log, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}


#[utoipa::path(
    delete,
    path = "/{route_type}/{media_id}/logs/{log_id}",
    params(LogDeleteParams),
    responses(
        (status = 200, description = "Log deleted"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The log was not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogDeleteParams>) -> impl IntoResponse {
    let result = services::logs::delete(params.media_id, params.log_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}
