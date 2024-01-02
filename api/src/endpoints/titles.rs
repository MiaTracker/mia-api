use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use services::infrastructure::SrvErr;
use views::titles::{TitleCreate, TitleCreateParams, TitleDeleteParams, TitleSetPrimaryParams};
use views::users::CurrentUser;
use crate::infrastructure::{ApiErr, AppState};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<TitleCreateParams>, Json(title): Json<TitleCreate>) -> impl IntoResponse {
    let result = services::titles::create(params.media_id, &title, params.route_type.into(), &user, &state.conn).await;
    match result {
        Ok(created) => { if created { StatusCode::CREATED.into_response() } else { StatusCode::OK.into_response() } }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn set_primary(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                         Path(params): Path<TitleSetPrimaryParams>) -> impl IntoResponse {
    let result = services::titles::set_primary(params.media_id, params.title_id, params.route_type.into(), &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<TitleDeleteParams>) -> impl IntoResponse {
    let result = services::titles::delete(params.media_id, params.title_id, params.route_type.into(), &user, &state.conn).await;
    match result {
        Ok(_) => { StatusCode::OK.into_response() }
        Err(err) => { <SrvErr as Into<ApiErr>>::into(err).into_response() }
    }
}