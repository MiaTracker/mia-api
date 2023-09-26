extern crate core;

mod config;
mod endpoints;
mod infrastructure;
mod middleware;

use std::env;
use std::net::SocketAddr;
use axum::Server;
use sea_orm::Database;
use tower::ServiceBuilder;
use crate::config::routes;
use crate::infrastructure::AppState;

pub async fn launch() {
    tracing_subscriber::fmt::init();

    let jwt_secret = env::var("JWT_SECRET").expect("JWT_SECRET not set!");

    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL not set!");
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    let state = AppState { conn, jwt_secret };

    let serv = ServiceBuilder::new()
        .layer(axum::middleware::from_fn_with_state(state.clone(), middleware::auth));
    let app = routes::build().layer(serv).merge(routes::build_anonymous()).with_state(state);
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));

    if let Ok(builder) = Server::try_bind(&addr) {
        tracing::debug!("Listening on {}", addr);
        let server = builder.serve(app.into_make_service());
        if let Err(err) = server.await {
            tracing::error!("Server error: {}", err);
        }
    } else {
        tracing::error!("Failed to bind to address {}", addr);
    }
}
