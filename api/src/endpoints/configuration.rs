use axum::http::StatusCode;
use axum::response::IntoResponse;
use crate::infrastructure::IntoApiResponse;
use views::configuration::ImagesConfiguration;
use views::api::ApiErrView;

#[utoipa::path(
    get,
    operation_id = "configuration::images",
    path = "/configuration/images",
    responses(
        (status = 200, description = "Current images configuration", body = ImagesConfiguration),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Vec<ApiErrView>]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Vec<ApiErrView>])
    ),
    security(("api_key" = []))
)]
pub async fn images() -> impl IntoResponse {
    let result = services::configuration::images().await;
    result.to_response(StatusCode::OK)
}