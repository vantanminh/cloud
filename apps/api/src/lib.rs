pub mod auth;
pub mod config;
pub mod error;
pub mod models;
pub mod security;
pub mod state;
pub mod workspaces;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde::Serialize;
use tower_http::trace::TraceLayer;

use crate::state::AppState;

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/auth/csrf", get(auth::csrf))
        .route("/auth/me", get(auth::me))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/workspaces", post(workspaces::create))
        .route("/workspaces/{slug}", get(workspaces::get));

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .nest("/api/v1", api)
        .layer(TraceLayer::new_for_http())
        .layer(state.config.cors_layer())
        .with_state(state)
}

async fn healthz() -> (StatusCode, Json<HealthResponse>) {
    (StatusCode::OK, Json(HealthResponse { status: "ok" }))
}

async fn readyz(State(state): State<AppState>) -> Result<Json<HealthResponse>, StatusCode> {
    sqlx::query("SELECT 1")
        .execute(&state.db)
        .await
        .map(|_| Json(HealthResponse { status: "ready" }))
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
