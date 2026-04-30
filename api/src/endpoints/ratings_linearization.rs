use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use axum::response::IntoResponse;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;
use crate::infrastructure::{AppState, IntoApiResponse};

#[derive(Deserialize, ToSchema)]
pub struct LinearizeRequest {
    #[schema(value_type = Option<String>)]
    pub user_uuid: Option<Uuid>,
    #[serde(default)]
    pub dry_run: bool,
}

#[utoipa::path(
    post,
    operation_id = "ratings_linearization::linearize",
    path = "/ratings/linearize",
    responses(
        (status = 200, description = "Linearization result"),
        (status = 401, description = "Authorization token was not provided, was invalid or the user is not an admin"),
        (status = 500, description = "An internal error occurred while processing the request")
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn linearize(
    state: State<AppState>,
    Json(req): Json<LinearizeRequest>,
) -> impl IntoResponse {
    services::ratings_linearization::linearize_ratings(&state.conn, req.user_uuid, req.dry_run)
        .await
        .to_response(StatusCode::OK)
}
