use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::genres::{GenreCreate, GenreCreateParams, GenreDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};
use views::app_tokens::AppTokenGenerate;
use views::api::ApiErrView;

#[utoipa::path(
    post,
    operation_id = "genres::create",
    path = "/{route_type}/{media_id}/genres",
    params(GenreCreateParams),
    request_body = AppTokenGenerate,
    responses(
        (status = 200, description = "Genre was already assigned to the media"),
        (status = 201, description = "Genre was added to the media"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "Media not found", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<GenreCreateParams>, Json(genre): Json<GenreCreate>) -> impl IntoResponse {
    let result = services::genres::create(params.media_id, &genre, params.route_type.into(), &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

#[utoipa::path(
    delete,
    operation_id = "genres::delete",
    path = "/{route_type}/{media_id}/genres/{genre_id}",
    params(GenreDeleteParams),
    responses(
        (status = 200, description = "Genre was removed from media"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "Media not found", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<GenreDeleteParams>) -> impl IntoResponse {
    let result = services::genres::delete(params.media_id, params.genre_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "genres::index",
    path = "/genres",
    responses(
        (status = 200, description = "All genres of user's media", body = [String]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::genres::index(None, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}