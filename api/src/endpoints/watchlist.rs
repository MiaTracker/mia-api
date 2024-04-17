use axum::{Extension, Json};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use views::media::{PageReq, SearchQuery};
use views::users::CurrentUser;
use views::watchlist::WatchlistParams;

use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/watchlist/add",
    request_body = WatchlistParams,
    responses(
        (status = 200, description = "Media already on watchlist"),
        (status = 201, description = "Media added to watchlist"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The media was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn add(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Json(params): Json<WatchlistParams>) -> impl IntoResponse {
    let result = services::watchlist::add(params.media_id, &user, &state.conn).await;
    result.map_to_status(|res| {
        if *res { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

#[utoipa::path(
    get,
    path = "/watchlist",
    params(PageReq),
    responses(
        (status = 200, description = "All media on watchlist", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::watchlist::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/watchlist/search",
    params(PageReq),
    request_body = SearchQuery,
    responses(
        (status = 200, description = "Filtered media on watchlist", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::watchlist::search(search, params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    path = "/watchlist/remove",
    request_body = WatchlistParams,
    responses(
        (status = 200, description = "Media successfully removed from watchlist"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The media was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn remove(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(params): Json<WatchlistParams>) -> impl IntoResponse {
    let result = services::watchlist::remove(params.media_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}