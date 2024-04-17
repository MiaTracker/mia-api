extern crate core;

mod config;
mod endpoints;
mod infrastructure;
mod middleware;
mod openapi;

use std::{env, fs};
use std::net::SocketAddr;
use sea_orm::Database;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use crate::config::routes;
use crate::infrastructure::AppState;
use crate::openapi::ApiDoc;

pub async fn launch() {
    tracing_subscriber::fmt::init();
    services::infrastructure::initialize().await;

    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET not set!");

    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL not set!");
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    let state = AppState { conn, jwt_secret };

    let admin_serv = ServiceBuilder::new()
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::auth))
        .layer(axum::middleware::from_fn(middleware::admin));
    let user_serv = ServiceBuilder::new()
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::auth))
        .layer(axum::middleware::from_fn(middleware::user));
    let auth_serv = ServiceBuilder::new()
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::auth));
    let cors_serv = ServiceBuilder::new()
        .layer(middleware::cors::build());
    let app = routes::build_admin().layer(admin_serv)
        .merge(routes::build_bot().layer(auth_serv))
        .merge(routes::build().layer(user_serv))
        .merge(routes::build_anonymous())
        .layer(cors_serv).with_state(state)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()));
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    if let Ok(listener) = TcpListener::bind(&addr).await {
        tracing::debug!("Listening on {}", addr);
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("Server error: {}", err);
        }
    } else {
        tracing::error!("Failed to bind to address {}", addr);
    }
}
