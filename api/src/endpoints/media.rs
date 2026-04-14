use axum::{Extension, Json};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::MaybeRouteType;
use views::media::{MediaByIdsQuery, PageReq, SearchParams, SearchQuery, MediaIndex, SearchResults};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};
use views::api::ApiErrView;

#[utoipa::path(
    get,
    operation_id = "media::index",
    path = "/media",
    params(PageReq),
    responses(
        (status = 200, description = "All media indexes", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::media::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    operation_id = "media::search",
    path = "/media/search",
    params(SearchParams),
    request_body = SearchQuery,
    responses(
        (status = 200, description = "Media indexes matching the search criteria", body = SearchResults),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::All.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    operation_id = "media::index_by_ids",
    path = "/media/by_ids",
    params(SearchParams),
    request_body = MediaByIdsQuery,
    responses(
        (status = 200, description = "Media indexes for the requested IDs matching the query", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn search_by_ids(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Query(params): Query<SearchParams>, Json(req): Json<MediaByIdsQuery>) -> impl IntoResponse {
    let result = services::media::search_by_ids(req, params.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}