use axum::{Extension, Json};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::logset::LogsetCreate;
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Json(logset): Json<LogsetCreate>) -> impl IntoResponse {
    let result = services::logset::create(&logset, &user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}