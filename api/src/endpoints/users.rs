use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use axum::response::IntoResponse;
use views::users::{CurrentUser, PasswordChange, UserDeleteParams, UserLogin, UserRegistration};
use crate::infrastructure::{AppState, IntoApiResponse};

#[utoipa::path(
    post,
    path = "/users/register",
    request_body = UserRegistration,
    responses(
        (status = 201, description = "User created"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn register(state: State<AppState>, Json(user): Json<UserRegistration>) -> impl IntoResponse {
    let result = services::users::register(&user, &state.conn).await;
    result.to_response(StatusCode::CREATED)
}

#[utoipa::path(
    post,
    path = "/users/login",
    request_body = UserLogin,
    responses(
        (status = 200, description = "User logged in successfully", body = UserToken),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    )
)]
pub async fn login(state: State<AppState>, Json(user): Json<UserLogin>) -> impl IntoResponse {
    let result = services::users::login(&user, &state.jwt_secret, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    get,
    path = "/users/profile",
    responses(
        (status = 200, description = "User profile data", body = UserProfile),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = []))
)]
pub async fn profile(Extension(user): Extension<CurrentUser>) -> impl IntoResponse {
    let result = services::users::profile(&user);
    (StatusCode::OK, Json(result)).into_response()
}

#[utoipa::path(
    get,
    path = "/users",
    responses(
        (status = 200, description = "User profile data", body = [UserIndex]),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn index(state: State<AppState>) -> impl IntoResponse {
    let result = services::users::index(&state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    patch,
    path = "/users/password",
    request_body = PasswordChange,
    responses(
        (status = 200, description = "User password changed successfully"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn change_password(state: State<AppState>, Extension(user): Extension<CurrentUser>,
                             Json(params): Json<PasswordChange>) -> impl IntoResponse {
    let result = services::users::change_password(params, &user, &state.conn).await;
    result.to_response(StatusCode::OK)
}

#[utoipa::path(
    delete,
    path = "/users/{uuid}",
    params(UserDeleteParams),
    responses(
        (status = 200, description = "User deleted"),
        (status = 400, description = "The request is invalid", body = [Error]),
        (status = 401, description = "Authorization token was not provided or is invalid", body = [Error]),
        (status = 404, description = "The user was not found"),
        (status = 500, description = "An internal error occurred while processing the request", body = [Error])
    ),
    security(("api_key" = ["admin"]))
)]
pub async fn delete(state: State<AppState>, Path(params): Path<UserDeleteParams>) -> impl IntoResponse {
    let result = services::users::delete(params.uuid, &state.conn).await;
    result.to_response(StatusCode::OK)
}