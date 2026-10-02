pub mod app_services;
pub mod auth;
pub mod cluster;
pub mod cluster_kubernetes;
pub mod config;
pub mod database;
pub mod error;
pub mod github;
pub mod html_pages;
pub mod knotree_registry;
pub mod kong;
pub mod limits;
pub mod mcp;
pub mod metrics;
pub mod models;
pub mod projects;
pub mod public_access;
pub mod redis_resources;
pub mod registry_consent;
pub mod resources;
pub mod security;
pub mod sso;
pub mod state;
pub mod workspaces;

#[cfg(test)]
pub mod test_support;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
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
        .route("/auth/knotree-registry/callback", get(registry_consent::callback))
        .route("/workspaces/{workspace_id}/projects/{project_slug}/registry-connections/authorize", post(registry_consent::start))
        .route("/auth/sso/config", get(sso::configuration))
        .route("/auth/sso/start", get(sso::start))
        .route("/auth/sso/callback", get(sso::callback))
        .route("/auth/csrf", get(auth::csrf))
        .route("/auth/me", get(auth::me))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/github/start", get(github::start))
        .route("/auth/github/callback", get(github::callback))
        .route("/auth/github/status", get(github::status))
        .route("/auth/github/disconnect", post(github::disconnect))
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/registry-connections",
            get(knotree_registry::list_connections).post(knotree_registry::create_connection),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/registry-connections/{connection_id}",
            patch(knotree_registry::update_connection)
                .delete(knotree_registry::revoke_connection),
        )
        .route(
            "/public/webhooks/knotree-registry",
            post(knotree_registry::webhook).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/oauth/register", post(mcp::register_client))
        .route("/oauth/token", post(mcp::token))
        .route("/mcp", post(mcp::mcp_endpoint))
        .route("/workspaces", post(workspaces::create))
        .route("/workspaces/{workspace_id}", get(workspaces::get));

    let api = api
        .route(
            "/workspaces/{workspace_id}/projects",
            get(projects::list).post(projects::create),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}",
            get(projects::get),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources",
            get(resources::list).post(resources::create),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/retry",
            post(resources::retry),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/deployments/{deployment_id}/events",
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
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/logs",
            get(app_services::logs),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/metrics",
            get(app_services::metrics),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/auto-deploy",
            patch(app_services::update_auto_deploy),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/registry-connection",
            patch(app_services::update_registry_connection),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/registry-deploys",
            get(app_services::registry_deploys),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/database",
            patch(app_services::update_database_connection),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/public-access",
            patch(app_services::update_public_access),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/html",
            get(html_pages::get_index_html).patch(app_services::update_html_page),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}/html-analytics",
            get(html_pages::analytics_summary),
        )
        .route(
            "/public/html-pages/github-push",
            post(html_pages::github_push_webhook),
        )
        .route(
            "/public/html-pages/{app_service_id}/analytics.js",
            get(html_pages::analytics_script),
        )
        .route(
            "/public/html-pages/{app_service_id}/events",
            post(html_pages::collect_event),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services/{app_service_id}",
            patch(app_services::update),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/redis",
            get(redis_resources::list).post(redis_resources::create),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/app-services",
            get(app_services::list).post(app_services::create),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/database/tables",
            get(database::list_tables).post(database::create_table),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/database/table-data",
            get(database::table_data),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/database/stats",
            get(database::stats),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/database/metrics",
            get(database::metrics),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/database/config",
            get(database::config),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/resources/{resource_id}/database/query",
            post(database::execute_query),
        );

    Router::new()
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
        .route(
            "/internal/public-traffic/{app_service_id}",
            post(app_services::record_kong_public_traffic),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            get(mcp::well_known_authorization_server),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(mcp::well_known_protected_resource),
        )
        .route("/oauth/authorize", get(mcp::authorize).post(mcp::authorize_submit))
        .route("/oauth/register", post(mcp::register_client))
        .route("/oauth/token", post(mcp::token))
        .route("/mcp", post(mcp::mcp_endpoint))
        .nest("/api/v1", api)
        .fallback(app_services::public_domain_fallback)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_services::public_domain_router,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| {
            tracing::info_span!("http", method = %request.method(), path = request.uri().path())
        }))
        .layer(state.config.cors_layer())
        // Outer so page-* origins can POST analytics without opening credentialed dashboard CORS.
        .layer(middleware::from_fn(html_pages::public_cors))
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
