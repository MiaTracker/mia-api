use axum::Json;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use crate::infrastructure::ApiErr;

pub async fn images() -> impl IntoResponse {
    let result = services::configuration::images().await;
    match result {
        Ok(config) => { Json(config).into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}