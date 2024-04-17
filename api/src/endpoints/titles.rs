use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::titles::{TitleCreate, TitleCreateParams, TitleDeleteParams, TitleSetPrimaryParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/{route_type}/{media_id}/titles",
    params(TitleCreateParams),
    request_body = TitleCreate,
    responses(
        (status = 201, description = "Title created"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<TitleCreateParams>, Json(title): Json<TitleCreate>) -> impl IntoResponse {
    let result = services::titles::create(params.media_id, &title, params.route_type.into(), &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

#[utoipa::path(
    post,
    path = "/{route_type}/{media_id}/titles/{title_id}/primary",
    params(TitleSetPrimaryParams),
    responses(
        (status = 200, description = "Title set as primary"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The title was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn set_primary(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                         Path(params): Path<TitleSetPrimaryParams>) -> impl IntoResponse {
    let result = services::titles::set_primary(params.media_id, params.title_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/{route_type}/{media_id}/titles/{title_id}",
    params(TitleDeleteParams),
    responses(
        (status = 200, description = "Title deleted"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The title was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<TitleDeleteParams>) -> impl IntoResponse {
    let result = services::titles::delete(params.media_id, params.title_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}