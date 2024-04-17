use axum::{Extension, Json};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::MaybeRouteType;
use views::media::{PageReq, SearchParams, SearchQuery};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    get,
    path = "/media",
    params(PageReq),
    responses(
        (status = 200, description = "All media indexes", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::media::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/media/search",
    params(SearchParams),
    request_body = SearchQuery,
    responses(
        (status = 200, description = "Media indexes matching the search criteria", body = SearchResults),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::All.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}