use axum::Extension;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::MaybeRouteType;
use views::media::MediaSearchQueryParams;
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::media::index(&user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaSearchQueryParams>) -> impl IntoResponse {
    let result = services::media::search(params.query, MaybeRouteType::All.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}