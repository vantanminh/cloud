pub mod app_services;
pub mod auth;
pub mod cluster;
pub mod cluster_kubernetes;
pub mod config;
pub mod database;
pub mod error;
pub mod github;
pub mod metrics;
pub mod models;
pub mod projects;
pub mod resources;
pub mod security;
pub mod state;
pub mod workspaces;

use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    middleware,
    routing::{any, get, patch, post},
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
        .route("/auth/github/start", get(github::start))
        .route("/auth/github/callback", get(github::callback))
        .route("/auth/github/status", get(github::status))
        .route("/auth/github/disconnect", post(github::disconnect))
        .route("/workspaces", post(workspaces::create))
        .route("/workspaces/{slug}", get(workspaces::get));

    let api = api
        .route(
            "/workspaces/{workspace_slug}/projects",
            get(projects::list).post(projects::create),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}",
            get(projects::get),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources",
            get(resources::list).post(resources::create),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services/deployments/{deployment_id}/events",
            get(app_services::deployment_events),
        )
        .route(
            "/public/app-services/{app_service_id}",
            any(app_services::public_proxy_root),
        )
        .route(
            "/public/app-services/{app_service_id}/{*path}",
            any(app_services::public_proxy_path),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services/{app_service_id}/logs",
            get(app_services::logs),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services/{app_service_id}/metrics",
            get(app_services::metrics),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services/{app_service_id}/auto-deploy",
            patch(app_services::update_auto_deploy),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services/{app_service_id}/database",
            patch(app_services::update_database_connection),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services/{app_service_id}",
            patch(app_services::update),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/app-services",
            get(app_services::list).post(app_services::create),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources/{resource_id}/database/tables",
            get(database::list_tables).post(database::create_table),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources/{resource_id}/database/table-data",
            get(database::table_data),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources/{resource_id}/database/stats",
            get(database::stats),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources/{resource_id}/database/metrics",
            get(database::metrics),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources/{resource_id}/database/config",
            get(database::config),
        )
        .route(
            "/workspaces/{workspace_slug}/projects/{project_slug}/resources/{resource_id}/database/query",
            post(database::execute_query),
        );

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .nest("/api/v1", api)
        .fallback(app_services::public_domain_fallback)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_services::public_domain_router,
        ))
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
