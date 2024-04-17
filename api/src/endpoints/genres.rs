use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::genres::{GenreCreate, GenreCreateParams, GenreDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/{route_type}/{media_id}/genres",
    params(GenreCreateParams),
    request_body = AppTokenGenerate,
    responses(
        (status = 200, description = "Genre was already assigned to the media"),
        (status = 201, description = "Genre was added to the media"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 404, description = "Media not found", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
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
    path = "/{route_type}/{media_id}/genres/{genre_id}",
    params(GenreDeleteParams),
    responses(
        (status = 200, description = "Genre was removed from media"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "Media not found", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
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
    path = "/genres",
    responses(
        (status = 200, description = "All genres of user's media", body = [String]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::genres::index(None, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}