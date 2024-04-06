use axum::{Extension, Json};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::api::MaybeRouteType;
use views::media::{PageReq, SearchParams, SearchQuery};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<PageReq>) -> impl IntoResponse {
    let result = services::media::index(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

pub async fn search(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Query(params): Query<SearchParams>, Json(search): Json<SearchQuery>) -> impl IntoResponse {
    let result = services::media::search(search, params.committed, params.into(), MaybeRouteType::All.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}