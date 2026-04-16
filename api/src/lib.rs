extern crate core;

mod config;
mod endpoints;
mod infrastructure;
mod middleware;
mod openapi;

use std::net::SocketAddr;
use sea_orm::Database;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use ::infrastructure::config;
use crate::config::routes;
use crate::infrastructure::AppState;
use crate::openapi::ApiDoc;

pub async fn launch() {
    services::infrastructure::initialize().await;

    let jwt_secret = config().jwt.secret.clone();

    let db_url = config().db.connection_url.clone();
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    let scheduler_conn = conn.clone();
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

    // Spawn the built-in refresh scheduler if configured
    if let Some(interval) = &config().tmdb.refresh_interval {
        let interval = interval.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(interval);
            loop {
                interval.tick().await;
                match services::refresh::refresh(&scheduler_conn).await {
                    Ok(r) => tracing::info!("Scheduled refresh complete. Updated: {}, Errors: {}", r.updated, r.errors),
                    Err(e) => tracing::error!("Scheduled refresh failed: {}", e),
                }
            }
        });
    }

    if let Ok(listener) = TcpListener::bind(&addr).await {
        tracing::debug!("Listening on {}", addr);
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("Server error: {}", err);
        }
    } else {
        tracing::error!("Failed to bind to address {}", addr);
    }
}
