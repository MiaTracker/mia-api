use axum::{Extension, Json};
use axum::extract::State;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn index(state: State<AppState>, Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::movies::index(&user, &state.conn).await;
    match result {
        Ok(movies) => { Json(movies).into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}