use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::tags::{TagCreate, TagCreateParams, TagDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/{route_type}/{media_id}/tags",
    params(TagCreateParams),
    request_body = TagCreate,
    responses(
        (status = 200, description = "Tag already attached to the media"),
        (status = 201, description = "Tag created"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(params): Path<TagCreateParams>, Json(tag): Json<TagCreate>) -> impl IntoResponse {
    let result = services::tags::create(params.media_id, tag, params.route_type.into(), &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

#[utoipa::path(
    delete,
    path = "/{route_type}/{media_id}/tags/{tag_id}",
    params(TagDeleteParams),
    responses(
        (status = 200, description = "Tag deleted"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The tag had not been added to the media"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(params): Path<TagDeleteParams>) -> impl IntoResponse {
    let result = services::tags::delete(params.media_id, params.tag_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}