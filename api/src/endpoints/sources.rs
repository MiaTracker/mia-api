use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::sources::{SourceCreate, SourceCreateParams, SourceDeleteParams, SourceDetailsParams, SourceIndexParams, SourceUpdate, SourceUpdateParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/{route_type}/{media_id}/sources",
    params(SourceCreateParams),
    request_body = SourceCreate,
    responses(
        (status = 201, description = "Source created"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<SourceCreateParams>, Json(source): Json<SourceCreate>) -> impl IntoResponse {
    let result = services::sources::create(params.media_id, &source, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

#[utoipa::path(
    get,
    path = "/{route_type}/{media_id}/sources",
    params(SourceIndexParams),
    responses(
        (status = 200, description = "All sources of the media", body = [Source]),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The media was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<SourceIndexParams>) -> impl IntoResponse {
    let result = services::sources::index(params.media_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/{route_type}/{media_id}/sources/{source_id}",
    params(SourceDetailsParams),
    responses(
        (status = 200, description = "Source details", body = Source),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The source was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<SourceDetailsParams>) -> impl IntoResponse {
    let result = services::sources::details(params.media_id, params.route_type.into(), params.source_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/{route_type}/{media_id}/sources/{source_id}",
    params(SourceUpdateParams),
    request_body = SourceUpdate,
    responses(
        (status = 200, description = "Source updated"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The source was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<SourceUpdateParams>, Json(source): Json<SourceUpdate>) -> impl IntoResponse {
    let result = services::sources::update(params.media_id, params.source_id, &source, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/{route_type}/{media_id}/sources/{source_id}",
    params(SourceDeleteParams),
    responses(
        (status = 200, description = "Source deleted"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The source was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(params): Path<SourceDeleteParams>) -> impl IntoResponse {
    let result = services::sources::delete(params.media_id, params.source_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}
