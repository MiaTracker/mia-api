use axum::{Extension, Json};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::media::MediaSearchQueryParams;
use views::users::CurrentUser;
use views::watchlist::WatchlistParams;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn add(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Json(params): Json<WatchlistParams>) -> impl IntoResponse {
    let result = services::watchlist::add(params.media_id, &user, &state.conn).await;
    result.map_to_status(|res| {
        if *res { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::watchlist::index(&user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaSearchQueryParams>) -> impl IntoResponse {
    let result = services::watchlist::search(params.query, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn remove(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(params): Json<WatchlistParams>) -> impl IntoResponse {
    let result = services::watchlist::remove(params.media_id, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}