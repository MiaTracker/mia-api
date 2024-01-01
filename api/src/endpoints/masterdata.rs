use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use crate::infrastructure::{ApiErr, AppState};

pub async fn refresh(state: State<AppState>) -> impl IntoResponse {
    let result = services::masterdata::refresh(&state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}