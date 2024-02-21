use axum::Extension;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn stats(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::statistics::stats(&user, &state.conn).await;
    result.to_response(StatusCode::OK)
}