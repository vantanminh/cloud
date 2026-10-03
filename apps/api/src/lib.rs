pub mod app_services;
pub mod auth;
pub mod cluster;
pub mod cluster_kubernetes;
pub mod config;
pub mod database;
pub mod error;
pub mod github;
pub mod html_pages;
pub mod images;
pub mod knotree_registry;
pub mod kong;
pub mod limits;
pub mod mcp;
pub mod metrics;
pub mod models;
pub mod projects;
pub mod public_access;
pub mod redis_resources;
pub mod registry_accounts;
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
    routing::{any, delete, get, patch, post},
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
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/registry-connections/from-account",
            post(registry_accounts::import_into_project),
        )
        .route(
            "/integrations/knotree-registry",
            get(registry_accounts::status),
        )
        .route(
            "/integrations/knotree-registry/repositories",
            get(registry_accounts::repositories),
        )
        .route(
            "/integrations/knotree-registry/repositories/{*repository}",
            get(registry_accounts::repository_tags),
        )
        .route("/auth/sso/config", get(sso::configuration))
        .route("/auth/sso/start", get(sso::start))
        .route("/auth/sso/callback", get(sso::callback))
        .route("/auth/csrf", get(auth::csrf))
        .route("/auth/me", get(auth::me))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/github/start", get(github::start))
        .route("/auth/github/callback", get(github::callback))
        .route("/auth/github/status", get(github::status))
        .route("/auth/github/disconnect", post(github::disconnect))
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/registry-connections",
            get(knotree_registry::list_connections),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/registry-connections/{connection_id}",
            axum::routing::delete(knotree_registry::revoke_connection),
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
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores",
            get(images::list_stores).post(images::create_store),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}",
            patch(images::update_store),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}/keys",
            get(images::list_keys).post(images::create_key),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}/keys/{key_id}/revoke",
            post(images::revoke_key),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}/objects",
            get(images::session_list_objects)
                .post(images::session_upload_object)
                .layer(DefaultBodyLimit::max(images::UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}/objects/{image_id}",
            delete(images::session_delete_object),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}/objects/{image_id}/sign",
            post(images::session_sign_object),
        )
        .route(
            "/workspaces/{workspace_id}/projects/{project_slug}/image-stores/{store_id}/folders",
            delete(images::session_delete_folder),
        )
        .route("/images/store", get(images::developer_store))
        .route(
            "/images/objects",
            get(images::list_objects)
                .post(images::upload_object)
                .layer(DefaultBodyLimit::max(images::UPLOAD_BODY_LIMIT)),
        )
        .route(
            "/images/objects/{image_id}",
            delete(images::delete_object),
        )
        .route(
            "/images/objects/{image_id}/sign",
            post(images::sign_object),
        )
        .route("/images/folders", delete(images::delete_folder));

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
        .route(
            "/images/v1/{store_id}/{image_id}",
            get(images::serve),
        )
        .fallback(app_services::public_domain_fallback)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            app_services::public_domain_router,
        ))
        .layer(TraceLayer::new_for_http().make_span_with(|request: &axum::http::Request<axum::body::Body>| {
            tracing::info_span!("http", method = %request.method(), path = request.uri().path())
        }))
        .layer(state.config.cors_layer())
        // Outer so browser image clients and page analytics can call from any origin
        // without opening credentialed dashboard CORS.
        .layer(middleware::from_fn(images::public_cors))
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
