use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::{MaybeRouteType, RouteType};
use views::images::{BackdropUpdate, ImagesUpdate, PosterUpdate, ImageCandidates};
use views::media::{MediaCreateParams, SearchQuery, MediaSourceCreate, MediaSourceDelete, MediaType, SearchParams, PageReq, MediaIndex, SearchResults, SeriesDeletePathParams};
use views::series::{SeriesDetails, SeriesMetadata};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};
use views::api::ApiErrView;
use views::images::ImageCandidate;

#[utoipa::path(
    post,
    operation_id = "series::create",
    path = "/series",
    params(MediaCreateParams),
    responses(
        (status = 200, description = "Series already exists", body = i32),
        (status = 201, description = "Series created", body = i32),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::series::create(params.tmdb_id, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

#[utoipa::path(
    post,
    operation_id = "series::create_w_source",
    path = "/series/source_create",
    request_body = MediaSourceCreate,
    responses(
        (status = 200, description = "Series already exists", body = i32),
        (status = 201, description = "Series created", body = i32),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn create_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceCreate>) -> impl IntoResponse {
    let result = services::media::create_w_source(json, MediaType::Series, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

#[utoipa::path(
    get,
    operation_id = "series::index",
    path = "/series",
    params(PageReq),
    responses(
        (status = 200, description = "All series indexes", body = [MediaIndex]),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::series::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)

}

#[utoipa::path(
    post,
    operation_id = "series::search",
    path = "/series/search",
    params(SearchParams),
    request_body = SearchQuery,
    responses(
        (status = 200, description = "Series indexes matching the search criteria", body = SearchResults),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::Series.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "series::genres",
    path = "/series/genres",
    responses(
        (status = 200, description = "All genres of user's series", body = [String]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn genres(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::genres::index(Some(MediaType::Series), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "series::details",
    path = "/series/{series_id}",
    params(
        ("series_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "Series details", body = SeriesDetails),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::details(series_id, &user, &state.conn).await;
    result.map_to_response(|series: &Option<SeriesDetails>| {
        match series {
            None => { (StatusCode::NOT_FOUND, None) }
            Some(details) => { (StatusCode::OK, Some(details)) }
        }
    })
}

#[utoipa::path(
    get,
    operation_id = "series::metadata",
    path = "/series/{series_id}/metadata",
    params(
        ("series_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "Series metadata", body = SeriesMetadata),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn metadata(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::metadata(series_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "series::on_watchlist",
    path = "/series/{series_id}/on_watchlist",
    params(
        ("series_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "Weather the series is currently on watchlist", body = bool),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn on_watchlist(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::on_watchlist(series_id, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    operation_id = "series::update",
    path = "/series/{series_id}/metadata",
    params(
        ("series_id" = i32, Path, )
    ),
    request_body = SeriesMetadata,
    responses(
        (status = 200, description = "Series metadata updated"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>, Json(metadata): Json<SeriesMetadata>) -> impl IntoResponse {
    let result = services::series::update(series_id, metadata, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "series::backdrops",
    path = "/series/{series_id}/backdrops",
    params(
        ("series_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "All backdrops of the series", body = Vec<ImageCandidate>),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn backdrops(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::backdrops(series_id, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    operation_id = "series::update_backdrop",
    path = "/series/{series_id}/backdrops/default",
    params(
        ("series_id" = i32, Path, )
    ),
    request_body = ImagesUpdate,
    responses(
        (status = 200, description = "Default series backdrop changed"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update_backdrop(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                              Path(series_id): Path<i32>, Json(json): Json<BackdropUpdate>) -> impl IntoResponse {
    let result = services::media::update_backdrop(series_id, MediaType::Series, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "series::posters",
    path = "/series/{series_id}/posters",
    params(
        ("series_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "All posters of the series", body = Vec<ImageCandidate>),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn posters(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::posters(series_id, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    operation_id = "series::update_poster",
    path = "/series/{series_id}/posters/default",
    params(
        ("series_id" = i32, Path, )
    ),
    request_body = ImagesUpdate,
    responses(
        (status = 200, description = "Default series poster changed"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update_poster(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                            Path(series_id): Path<i32>, Json(json): Json<PosterUpdate>) -> impl IntoResponse {
    let result = services::media::update_poster(series_id, MediaType::Series, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    operation_id = "series::images",
    path = "/series/{series_id}/images",
    params(
        ("series_id" = i32, Path, )
    ),
    responses(
        (status = 200, description = "All images of the series", body = ImageCandidates),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn images(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::images(series_id, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    operation_id = "series::update_images",
    path = "/series/{series_id}/images",
    params(
        ("series_id" = i32, Path, )
    ),
    request_body = ImagesUpdate,
    responses(
        (status = 200, description = "Default series images changed"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn update_images(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Path(series_id): Path<i32>, Json(json): Json<ImagesUpdate>) -> impl IntoResponse {
    let result = services::media::update_images(series_id, MediaType::Series, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    operation_id = "series::lock_property",
    path = "/series/{series_id}/{property}/lock",
    params(
        ("series_id" = i32, Path, ),
        ("property" = &str, Path, )
    ),
    responses(
        (status = 200, description = "Property was locked"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn lock_property(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Path((series_id, property)): Path<(i32, String)>) -> impl IntoResponse {
    let result = services::series::lock(series_id, property, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    operation_id = "series::unlock_property",
    path = "/series/{series_id}/{property}/unlock",
    params(
        ("series_id" = i32, Path, ),
        ("property" = &str, Path, )
    ),
    responses(
        (status = 200, description = "Property was unlocked"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn unlock_property(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Path((series_id, property)): Path<(i32, String)>) -> impl IntoResponse {
    let result = services::series::unlock(series_id, property, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    operation_id = "series::delete",
    path = "/series/{series_id}",
    params(SeriesDeletePathParams),
    responses(
        (status = 200, description = "Series deleted"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(path): Path<SeriesDeletePathParams>) -> impl IntoResponse {
    let result = services::media::delete(path.series_id, RouteType::Series.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    post,
    operation_id = "series::delete_w_source",
    path = "/series/source_delete",
    request_body = MediaSourceDelete,
    responses(
        (status = 200, description = "Series deleted"),
        (status = 400, description = "The request is invalid", body = [Vec<ApiErrView>]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 404, description = "The series was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn delete_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceDelete>) -> impl IntoResponse {
    let result = services::media::delete_w_source(json.tmdb_id, json.source, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}