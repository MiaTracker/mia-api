use axum::Extension;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::media::MediaCreateParams;
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>, Query(params): Query<MediaCreateParams>) -> impl IntoResponse {
    let result = services::series::create(params.tmdb_id, &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::CREATED.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}