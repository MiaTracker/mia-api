use axum::{Extension, Json};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use views::genres::{GenreCreate, GenreCreateParams, GenreDeleteParams};
use views::users::CurrentUser;
use crate::infrastructure::{AppState, IntoApiResponse};

pub async fn create(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<GenreCreateParams>, Json(genre): Json<GenreCreate>) -> impl IntoResponse {
    let result = services::genres::create(params.media_id, &genre, params.route_type.into(), &user, &state.conn).await;
    result.map_to_status(|&created| {
        if created { StatusCode::CREATED }
        else { StatusCode::OK }
    })
}

pub async fn delete(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                    Path(params): Path<GenreDeleteParams>) -> impl IntoResponse {
    let result = services::genres::delete(params.media_id, params.genre_id, params.route_type.into(), &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}