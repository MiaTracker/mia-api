use axum::Extension;
use axum::extract::{Path, Json, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::logs::{LogCreate, LogDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(media_id): Path<i32>, Json(log): Json<LogCreate>) -> impl IntoResponse {
    let result = services::logs::create(media_id, &log, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::CREATED.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<LogDeleteParams>) -> impl IntoResponse {
    let result = services::logs::delete(params.media_id, params.log_id, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}