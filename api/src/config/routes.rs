use axum::Router;
use axum::routing::post;
use crate::endpoints::users;
use crate::infrastructure::AppState;

pub fn build() -> Router<AppState>
{
    Router::new()
}

pub fn build_anonymous() -> Router<AppState> {
    Router::new()
        .route("/users/register", post(users::register))
        .route("/users/login", post(users::login))
}