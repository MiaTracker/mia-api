extern crate core;

mod config;
mod endpoints;
mod infrastructure;
mod middleware;
mod openapi;

use std::net::SocketAddr;
use std::sync::Arc;
use sea_orm::Database;
use tokio::net::TcpListener;
use tower::ServiceBuilder;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use ::infrastructure::config;
use services::infrastructure::{Clock, FsImageStore, ImageStore, Llm, OpenRouterClient, OsRandom, Random, SystemClock, Tmdb, TmdbClient};
use services::chat::ExchangeRegistry;
use services::jobs::{JobDeps, Jobs, TokioJobs};
use crate::config::routes;
use crate::infrastructure::AppState;
use crate::openapi::ApiDoc;

pub async fn launch() {
    let tmdb: Arc<dyn Tmdb> = Arc::new(TmdbClient::new(config().tmdb.authorization_token.clone()));
    services::infrastructure::initialize(tmdb.as_ref()).await;

    let jwt_secret = config().jwt.secret.clone();

    let db_url = config().db.connection_url.clone();
    let conn = Database::connect(db_url.clone()).await
        .expect(format!("Failed to connect to database using connection string \"{}\"", db_url).as_str());

    let conn = Arc::new(conn);
    let scheduler_conn = conn.clone();
    let scheduler_tmdb = tmdb.clone();
    let image_store: Arc<dyn ImageStore> = Arc::new(FsImageStore::new(config().images.store_path.clone()));
    let scheduler_image_store = image_store.clone();
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);

    match services::chat::recover_interrupted(&conn, clock.as_ref()).await {
        Ok(count) => tracing::info!("Marked {} interrupted exchange(s) as failed", count),
        Err(err) => tracing::error!("Failed to recover interrupted exchanges: {}", err),
    }

    let openrouter = &config().openrouter;
    let llm: Arc<dyn Llm> = Arc::new(OpenRouterClient::new(
        openrouter.api_key.clone(),
        openrouter.model.clone(),
        openrouter.max_tokens,
        openrouter.reasoning.as_ref().map(|reasoning| reasoning.budget_tokens),
    ));
    let registry = Arc::new(ExchangeRegistry::new());
    let jobs: Arc<dyn Jobs> = Arc::new(TokioJobs::new(JobDeps {
        db: conn.clone(), store: image_store.clone(), llm: llm.clone(), registry: registry.clone(), clock: clock.clone(),
    }));
    let scheduler_jobs = jobs.clone();
    let scheduler_clock = clock.clone();
    let random: Arc<dyn Random> = Arc::new(OsRandom);
    let scheduler_random = random.clone();
    let state = AppState { conn, jwt_secret, tmdb, image_store, llm, registry, jobs, clock, random };

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
            interval.tick().await; // First tick finishes immediately
            loop {
                interval.tick().await;
                match services::refresh::refresh(&scheduler_conn, scheduler_tmdb.as_ref(), scheduler_image_store.as_ref(), scheduler_jobs.as_ref(), scheduler_clock.as_ref(), scheduler_random.as_ref()).await {
                    Ok(r) => tracing::info!("Scheduled refresh complete. Updates: {}, Errors: {}", r.updates, r.errors),
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
