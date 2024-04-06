use axum::{Extension, Json};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::{MaybeRouteType, RouteType};
use views::images::ImagesUpdate;
use views::media::{MediaCreateParams, MediaDeletePathParams, SearchQuery, MediaSourceCreate, MediaSourceDelete, MediaType, SearchParams, PageReq};
use views::series::{SeriesDetails, SeriesMetadata};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::series::create(params.tmdb_id, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

pub async fn create_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceCreate>) -> impl IntoResponse {
    let result = services::media::create_w_source(json, MediaType::Series, &user, &state.conn).await;
    result.map_to_status_and_result(|&res| {
        if res.0 { (StatusCode::CREATED, res.1) }
        else { (StatusCode::OK, res.1) }
    })
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::series::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)

}

pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::Series.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn genres(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::genres::index(Some(MediaType::Series), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn details(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::details(series_id, &user, &state.conn).await;
    result.map_to_response(|series: &Option<SeriesDetails>| {
        match series {
            None => { (StatusCode::NOT_FOUND, None) }
            Some(details) => { (StatusCode::OK, Some(details)) }
        }
    })
}

pub async fn metadata(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::series::metadata(series_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn on_watchlist(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::on_watchlist(series_id, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn update(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>, Json(metadata): Json<SeriesMetadata>) -> impl IntoResponse {
    let result = services::series::update(series_id, metadata, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn images(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(series_id): Path<i32>) -> impl IntoResponse {
    let result = services::media::images(series_id, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn update_images(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                           Path(series_id): Path<i32>, Json(json): Json<ImagesUpdate>) -> impl IntoResponse {
    let result = services::media::update_images(series_id, MediaType::Series, json, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>, Path(path): Path<MediaDeletePathParams>) -> impl IntoResponse {
    let result = services::media::delete(path.media_id, RouteType::Series.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn delete_w_source(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(json): Json<MediaSourceDelete>) -> impl IntoResponse {
    let result = services::media::delete_w_source(json.tmdb_id, json.source, MediaType::Series, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}