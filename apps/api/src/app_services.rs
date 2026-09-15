use std::{
    collections::BTreeMap,
    convert::Infallible,
    process::{Output, Stdio},
    time::Instant,
};

use anyhow::{Context, Result, bail};
use axum::{
    Json,
    body::{Body, to_bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, Request, StatusCode, header},
    middleware::Next,
    response::{
        Html, IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use futures_util::{StreamExt, stream};
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    time::{Duration, sleep},
};
use uuid::Uuid;

use crate::{
    auth,
    cluster::{self, PROVIDER_DOCKER},
    error::AppError,
    github::{self, GithubDockerCredentials},
    metrics::{
        MAX_METRIC_RESPONSE_POINTS, METRIC_RETENTION_SECONDS, METRIC_SAMPLE_INTERVAL_SECONDS,
        downsample_metric_points, has_system_metrics, metric_sample_concurrency,
        parse_metric_range, unix_timestamp,
    },
    models::{
        AppServiceDatabaseConnectionResponse, AppServiceDeploymentResponse, AppServiceLogsResponse,
        AppServiceMetricPoint, AppServiceMetricsResponse, AppServiceResponse,
        CreateAppServiceRequest, UpdateAppServiceAutoDeployRequest,
        UpdateAppServiceDatabaseRequest, UpdateAppServiceRequest,
    },
    projects, security,
    state::AppState,
};

const IMAGE_SOURCE_PUBLIC: &str = "public";
const IMAGE_SOURCE_GITHUB: &str = "github";
const STATUS_READY: &str = "ready";
const STATUS_PROVISIONING: &str = "provisioning";
const STATUS_ERROR: &str = "error";
const DEFAULT_APP_PORT: u16 = 3000;
const MAX_APP_SERVICES_PER_PROJECT: i64 = 6;
const AUTO_DEPLOY_INTERVAL_SECONDS: u64 = 60;
const MAX_PUBLIC_PROXY_BODY_BYTES: usize = 64 * 1024 * 1024;
const PROVISIONING_ERROR_MESSAGE: &str =
    "The Docker app service could not be deployed. Check the image and try again.";
const PUBLIC_DOMAIN_NOT_FOUND_HTML: &str = r#"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>App not found · Knotree</title>
    <style>
      :root { color-scheme: dark; font-family: Inter, ui-sans-serif, system-ui, sans-serif; }
      body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: #0b1020; color: #f7f8ff; }
      main { width: min(32rem, calc(100% - 3rem)); padding: 2rem; border: 1px solid #26304e; border-radius: 1.25rem; background: #121a31; box-shadow: 0 1.5rem 4rem #0006; }
      .mark { color: #91a7ff; font-size: .8rem; font-weight: 700; letter-spacing: .14em; text-transform: uppercase; }
      h1 { margin: 1rem 0 .75rem; font-size: clamp(2rem, 8vw, 3.25rem); line-height: 1; }
      p { color: #b7c0db; line-height: 1.6; }
      code { color: #d7def5; }
      a { display: inline-block; margin-top: 1rem; color: #aebcff; font-weight: 700; text-decoration: none; }
      a:hover { text-decoration: underline; }
    </style>
  </head>
  <body>
    <main>
      <div class="mark">Knotree · 404</div>
      <h1>App not found</h1>
      <p>There is no public app service assigned to this domain. Check the URL or ask the owner for a new link.</p>
      <a href="https://knotree.org">Back to Knotree</a>
    </main>
  </body>
</html>
"#;

#[derive(Debug, Clone, sqlx::FromRow)]
struct AppServiceRow {
    id: Uuid,
    public_subdomain: String,
    project_id: Uuid,
    name: String,
    image: String,
    image_source: String,
    app_port: i32,
    host: Option<String>,
    port: Option<i32>,
    container_name: Option<String>,
    status: String,
    error_message: Option<String>,
    database_resource_id: Option<Uuid>,
    auto_deploy_enabled: bool,
    deployed_image_digest: Option<String>,
    auto_deploy_checked_at: Option<OffsetDateTime>,
    auto_deploy_error: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AppServiceDeploymentRow {
    id: Uuid,
    status: String,
    current_step: String,
    logs: String,
    error_message: Option<String>,
    updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AppServiceLogsTarget {
    container_name: Option<String>,
    status: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AutoDeployCandidate {
    id: Uuid,
    image: String,
    container_name: Option<String>,
    github_connection_user_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
struct QueuedAutoDeployment {
    deployment_id: Uuid,
    service_id: Uuid,
    project_id: Uuid,
    image: String,
    app_port: u16,
    database_resource_id: Option<Uuid>,
    database: Option<DatabaseResourceRow>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AppServiceMetricHistoryRow {
    sample_timestamp: i64,
    cpu_percent: Option<f64>,
    memory_used_bytes: Option<i64>,
    memory_limit_bytes: Option<i64>,
    volume_used_bytes: Option<i64>,
    volume_capacity_bytes: Option<i64>,
    network_receive_bytes: Option<i64>,
    network_transmit_bytes: Option<i64>,
    disk_read_bytes: Option<i64>,
    disk_write_bytes: Option<i64>,
    public_network_receive_bytes: Option<i64>,
    public_network_transmit_bytes: Option<i64>,
    requests: Option<i64>,
    response_time_ms: Option<f64>,
    request_error_rate: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct AppServiceMetricsQuery {
    pub range: Option<String>,
}

const APP_SERVICE_COLUMNS: &str = "id, public_subdomain, project_id, name, image, image_source, app_port, host, port, container_name, status, error_message, database_resource_id, auto_deploy_enabled, github_connection_user_id, deployed_image_digest, auto_deploy_checked_at, auto_deploy_error";
const APP_SERVICE_LOG_TAIL_LINES: &str = "200";

#[derive(Debug, Clone, sqlx::FromRow)]
struct DatabaseResourceRow {
    id: Uuid,
    name: String,
    database_name: String,
    role_name: String,
    password_ciphertext: String,
}

const DATABASE_RESOURCE_COLUMNS: &str = "id, name, database_name, role_name, password_ciphertext";

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<AppServiceResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let services = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE project_id = $1 ORDER BY created_at ASC, id ASC"
    ))
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let databases = ready_database_resources(&state, project_id).await?;
    let mut response = Vec::with_capacity(services.len());
    for service in &services {
        let database = service
            .database_resource_id
            .and_then(|resource_id| databases.iter().find(|database| database.id == resource_id));
        let mut response_service = service.clone();
        if service.status == STATUS_READY {
            if let Some(container_name) = service.container_name.as_deref() {
                if let Ok(port) = current_app_service_port(
                    &state,
                    service.id,
                    container_name,
                    service.app_port,
                    service.port,
                )
                .await
                {
                    response_service.port = Some(i32::from(port));
                }
            }
        }
        response.push(app_service_response(
            &response_service,
            &state.config.app_service_public_host,
            state.config.bind_addr.port(),
            state.config.app_service_public_domain.as_deref(),
            &state.config.app_service_public_scheme,
            database,
            latest_deployment(&state.db, service.id).await?,
        )?);
    }
    Ok(Json(response))
}

pub async fn deployment_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, deployment_id)): Path<(String, String, Uuid)>,
) -> Result<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>, AppError>
{
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let deployment_exists = sqlx::query_scalar::<_, Uuid>(
        "SELECT d.id FROM app_service_deployments d JOIN project_app_services s ON s.id = d.app_service_id WHERE d.id = $1 AND s.project_id = $2",
    )
    .bind(deployment_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    if deployment_exists.is_none() {
        return Err(AppError::NotFound {
            code: "APP_SERVICE_DEPLOYMENT_NOT_FOUND",
            message: "The app service deployment could not be found.",
        });
    }

    let stream = stream::unfold(
        (state.db.clone(), deployment_id, None, false),
        |(db, deployment_id, last_updated, finished)| async move {
            if finished {
                return None;
            }
            loop {
                let deployment = sqlx::query_as::<_, AppServiceDeploymentRow>(
                    "SELECT id, status, current_step, logs, error_message, updated_at FROM app_service_deployments WHERE id = $1",
                )
                .bind(deployment_id)
                .fetch_optional(&db)
                .await;
                match deployment {
                    Ok(Some(deployment)) => {
                        let changed =
                            last_updated.map_or(true, |last| deployment.updated_at > last);
                        if changed {
                            let terminal = deployment.status != STATUS_PROVISIONING;
                            let next_state =
                                (db, deployment_id, Some(deployment.updated_at), terminal);
                            let event = Event::default().event("deployment").data(
                                serde_json::to_string(&deployment_response(&deployment))
                                    .unwrap_or_else(|_| "{}".to_owned()),
                            );
                            return Some((Ok(event), next_state));
                        }
                    }
                    Ok(None) => {
                        let response = AppServiceDeploymentResponse {
                            id: deployment_id,
                            status: STATUS_ERROR.to_owned(),
                            current_step: "Live log stream".to_owned(),
                            logs: vec!["The deployment record is no longer available.".to_owned()],
                            error_message: Some(
                                "The deployment record is no longer available.".to_owned(),
                            ),
                        };
                        let event = Event::default().event("deployment").data(
                            serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_owned()),
                        );
                        return Some((Ok(event), (db, deployment_id, last_updated, true)));
                    }
                    Err(error) => {
                        tracing::error!(deployment_id = %deployment_id, error = %error, "app service deployment event stream failed");
                        let response = AppServiceDeploymentResponse {
                            id: deployment_id,
                            status: STATUS_ERROR.to_owned(),
                            current_step: "Live log stream".to_owned(),
                            logs: vec!["The live deployment log stream failed.".to_owned()],
                            error_message: Some(
                                "Reconnect to view the saved deployment logs.".to_owned(),
                            ),
                        };
                        let event = Event::default().event("deployment").data(
                            serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_owned()),
                        );
                        return Some((Ok(event), (db, deployment_id, last_updated, true)));
                    }
                }
                sleep(Duration::from_millis(350)).await;
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}

pub async fn logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, app_service_id)): Path<(String, String, Uuid)>,
) -> Result<Json<AppServiceLogsResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let target = sqlx::query_as::<_, AppServiceLogsTarget>(
        "SELECT container_name, status FROM project_app_services WHERE id = $1 AND project_id = $2",
    )
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;

    let Some(container_name) = target.container_name.clone() else {
        return Ok(Json(AppServiceLogsResponse {
            app_service_id,
            container_name: None,
            status: target.status,
            running: false,
            lines: Vec::new(),
            message: Some("The app service container has not been deployed yet.".to_owned()),
        }));
    };

    if target.status != STATUS_READY {
        return Ok(Json(AppServiceLogsResponse {
            app_service_id,
            container_name: Some(container_name),
            status: target.status,
            running: false,
            lines: Vec::new(),
            message: Some("Runtime logs are available after the app service is ready.".to_owned()),
        }));
    }

    let running = docker_container_running(&state, &container_name).await?;
    let output = docker_raw(
        &state,
        [
            "logs".to_owned(),
            "--timestamps".to_owned(),
            "--tail".to_owned(),
            APP_SERVICE_LOG_TAIL_LINES.to_owned(),
            container_name.clone(),
        ],
    )
    .await
    .map_err(|error| {
        tracing::warn!(
            app_service_id = %app_service_id,
            container_name = %container_name,
            error = %error,
            "could not read app service container logs"
        );
        AppError::ServiceUnavailable {
            code: "APP_SERVICE_LOGS_UNAVAILABLE",
            message: "The app service logs are temporarily unavailable.",
        }
    })?;

    if !output.status.success() {
        tracing::warn!(
            app_service_id = %app_service_id,
            container_name = %container_name,
            error = %String::from_utf8_lossy(&output.stderr),
            "Docker could not read app service container logs"
        );
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_LOGS_UNAVAILABLE",
            message: "The app service logs are temporarily unavailable.",
        });
    }

    Ok(Json(AppServiceLogsResponse {
        app_service_id,
        container_name: Some(container_name),
        status: target.status,
        running,
        lines: docker_log_lines(&output),
        message: None,
    }))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PublicProxyTarget {
    app_port: i32,
    port: Option<i32>,
    container_name: Option<String>,
    status: String,
}

#[derive(Debug, Clone, Copy)]
struct PublicTrafficMetricValues {
    public_network_receive_bytes: Option<i64>,
    public_network_transmit_bytes: Option<i64>,
    requests: Option<i64>,
    response_time_ms: Option<f64>,
    request_error_rate: Option<f64>,
}

fn public_traffic_metric_values(
    state: &AppState,
    app_service_id: Uuid,
) -> PublicTrafficMetricValues {
    let traffic = state.public_app_service_traffic(app_service_id);
    let requests = traffic.requests;
    PublicTrafficMetricValues {
        public_network_receive_bytes: Some(
            i64::try_from(traffic.public_network_receive_bytes).unwrap_or(i64::MAX),
        ),
        public_network_transmit_bytes: Some(
            i64::try_from(traffic.public_network_transmit_bytes).unwrap_or(i64::MAX),
        ),
        requests: Some(i64::try_from(requests).unwrap_or(i64::MAX)),
        response_time_ms: (requests > 0).then(|| traffic.response_time_ms_total / requests as f64),
        request_error_rate: (requests > 0)
            .then(|| (traffic.request_errors.min(requests) as f64 / requests as f64) * 100.0),
    }
}

async fn current_app_service_port(
    state: &AppState,
    app_service_id: Uuid,
    container_name: &str,
    app_port: i32,
    stored_port: Option<i32>,
) -> Result<u16, AppError> {
    let app_port = u16::try_from(app_port)
        .map_err(|_| AppError::internal("invalid app service container port"))?;
    let current_port = docker_port(state, container_name, app_port, None)
        .await
        .map_err(|error| {
            tracing::warn!(
                app_service_id = %app_service_id,
                container_name,
                error = %error,
                "could not resolve the app service published port"
            );
            AppError::ServiceUnavailable {
                code: "APP_SERVICE_PUBLIC_UNAVAILABLE",
                message: "The app service is temporarily unavailable.",
            }
        })?;

    if stored_port != Some(i32::from(current_port)) {
        if let Err(error) = sqlx::query(
            "UPDATE project_app_services
             SET port = $1, updated_at = now()
             WHERE id = $2 AND status = $3 AND container_name = $4",
        )
        .bind(i32::from(current_port))
        .bind(app_service_id)
        .bind(STATUS_READY)
        .bind(container_name)
        .execute(&state.db)
        .await
        {
            tracing::warn!(
                app_service_id = %app_service_id,
                current_port,
                error = %error,
                "could not persist the refreshed app service published port"
            );
        }
    }

    Ok(current_port)
}

pub async fn public_proxy_root(
    State(state): State<AppState>,
    Path(app_service_id): Path<Uuid>,
    request: Request<Body>,
) -> Result<Response, AppError> {
    proxy_public_request(&state, app_service_id, request).await
}

pub async fn public_proxy_path(
    State(state): State<AppState>,
    Path((app_service_id, _path)): Path<(Uuid, String)>,
    request: Request<Body>,
) -> Result<Response, AppError> {
    proxy_public_request(&state, app_service_id, request).await
}

pub async fn public_domain_fallback(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response {
    if !is_public_domain_request(&state, &request) {
        return StatusCode::NOT_FOUND.into_response();
    }
    public_domain_proxy(State(state), request).await
}

pub async fn public_domain_router(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if is_public_domain_request(&state, &request) {
        return public_domain_proxy(State(state), request).await;
    }
    next.run(request).await
}

pub async fn public_domain_proxy(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response {
    let public_domain_request = is_public_domain_request(&state, &request);
    match proxy_public_domain_request(&state, request).await {
        Ok(response) => response,
        Err(error) if public_domain_request && error.is_not_found() => {
            public_domain_not_found_response()
        }
        Err(error) => error.into_response(),
    }
}

fn public_domain_not_found_response() -> Response {
    (StatusCode::NOT_FOUND, Html(PUBLIC_DOMAIN_NOT_FOUND_HTML)).into_response()
}

async fn proxy_public_domain_request(
    state: &AppState,
    request: Request<Body>,
) -> Result<Response, AppError> {
    let public_domain =
        state
            .config
            .app_service_public_domain
            .as_deref()
            .ok_or(AppError::NotFound {
                code: "APP_SERVICE_PUBLIC_DOMAIN_NOT_CONFIGURED",
                message: "Public app service domains are not configured.",
            })?;
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or(AppError::NotFound {
            code: "APP_SERVICE_DOMAIN_NOT_FOUND",
            message: "The public app service domain could not be resolved.",
        })?;
    let public_subdomain =
        public_subdomain_from_host(host, public_domain).ok_or(AppError::NotFound {
            code: "APP_SERVICE_DOMAIN_NOT_FOUND",
            message: "The public app service domain could not be resolved.",
        })?;
    let app_service_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM project_app_services WHERE public_subdomain = $1",
    )
    .bind(public_subdomain)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_DOMAIN_NOT_FOUND",
        message: "The public app service domain could not be resolved.",
    })?;

    proxy_public_request_with_mode(state, app_service_id, request, true).await
}

fn is_public_domain_request(state: &AppState, request: &Request<Body>) -> bool {
    let Some(public_domain) = state.config.app_service_public_domain.as_deref() else {
        return false;
    };
    let Some(host) = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    host_matches_public_domain(host, public_domain)
}

fn host_matches_public_domain(host: &str, public_domain: &str) -> bool {
    let Some(host) = host_without_port(host) else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let domain = public_domain.trim_end_matches('.').to_ascii_lowercase();
    !host.is_empty() && (host == domain || host.ends_with(&format!(".{domain}")))
}

fn public_subdomain_from_host(host: &str, public_domain: &str) -> Option<String> {
    let host = host_without_port(host)?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let domain = public_domain.trim_end_matches('.').to_ascii_lowercase();
    let suffix = format!(".{domain}");
    let subdomain = host.strip_suffix(&suffix)?;
    if subdomain.contains('.') || !is_valid_public_subdomain(subdomain) {
        return None;
    }
    Some(subdomain.to_owned())
}

fn host_without_port(host: &str) -> Option<&str> {
    let host = host.trim();
    if host.is_empty() || host.starts_with('[') {
        return None;
    }
    match host.rsplit_once(':') {
        Some((hostname, port)) if !hostname.contains(':') => {
            if port.is_empty() || port.parse::<u16>().is_err() {
                None
            } else {
                Some(hostname)
            }
        }
        Some(_) => None,
        None => Some(host),
    }
}

fn is_valid_public_subdomain(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 63
        && !value.starts_with('-')
        && !value.ends_with('-')
        && value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

fn new_public_subdomain() -> String {
    let random = Uuid::new_v4().simple().to_string();
    format!("app-{}", &random[..16])
}

async fn proxy_public_request(
    state: &AppState,
    app_service_id: Uuid,
    request: Request<Body>,
) -> Result<Response, AppError> {
    proxy_public_request_with_mode(state, app_service_id, request, false).await
}

async fn proxy_public_request_with_mode(
    state: &AppState,
    app_service_id: Uuid,
    request: Request<Body>,
    domain_request: bool,
) -> Result<Response, AppError> {
    let target = sqlx::query_as::<_, PublicProxyTarget>(
        "SELECT app_port, port, container_name, status
         FROM project_app_services
         WHERE id = $1",
    )
    .bind(app_service_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;
    if target.status != STATUS_READY {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_NOT_READY",
            message: "The app service is not ready to receive public traffic.",
        });
    }
    let Some(container_name) = target.container_name.as_deref() else {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_NOT_READY",
            message: "The app service is not ready to receive public traffic.",
        });
    };
    let port = current_app_service_port(
        state,
        app_service_id,
        container_name,
        target.app_port,
        target.port,
    )
    .await?;
    let started_at = Instant::now();
    let (parts, body) = request.into_parts();
    let request_body = to_bytes(body, MAX_PUBLIC_PROXY_BODY_BYTES)
        .await
        .map_err(|_| {
            state.record_public_app_service_request(
                app_service_id,
                0,
                0,
                started_at.elapsed().as_secs_f64() * 1_000.0,
                true,
            );
            AppError::BadRequest {
                code: "PUBLIC_REQUEST_TOO_LARGE",
                message: "The public request body is too large.",
            }
        })?;
    let request_bytes = request_body.len();
    let upstream_path = if domain_request {
        public_domain_upstream_path(&parts.uri)
    } else {
        public_upstream_path(&parts.uri)
    };
    let bind_host = url_host(&state.config.app_service_bind_address.to_string());
    let upstream_url = format!("http://{bind_host}:{port}{upstream_path}");
    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes()).map_err(|_| {
        AppError::BadRequest {
            code: "PUBLIC_METHOD_UNSUPPORTED",
            message: "The public request method is not supported.",
        }
    })?;
    let mut upstream_request = state.public_proxy_client.request(method, upstream_url);
    for (name, value) in &parts.headers {
        if should_forward_public_header(name) {
            upstream_request = upstream_request.header(name, value);
        }
    }
    let upstream_response = match upstream_request.body(request_body).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                app_service_id = %app_service_id,
                error = %error,
                "public app service request could not reach the container"
            );
            state.record_public_app_service_request(
                app_service_id,
                request_bytes,
                0,
                started_at.elapsed().as_secs_f64() * 1_000.0,
                true,
            );
            return Err(AppError::ServiceUnavailable {
                code: "APP_SERVICE_PUBLIC_UNAVAILABLE",
                message: "The app service is temporarily unavailable.",
            });
        }
    };
    let status = upstream_response.status();
    let response_headers = upstream_response.headers().clone();
    let response_body = match upstream_response.bytes().await {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(
                app_service_id = %app_service_id,
                error = %error,
                "public app service response could not be read"
            );
            state.record_public_app_service_request(
                app_service_id,
                request_bytes,
                0,
                started_at.elapsed().as_secs_f64() * 1_000.0,
                true,
            );
            return Err(AppError::ServiceUnavailable {
                code: "APP_SERVICE_PUBLIC_UNAVAILABLE",
                message: "The app service response is temporarily unavailable.",
            });
        }
    };
    let response_bytes = response_body.len();
    state.record_public_app_service_request(
        app_service_id,
        request_bytes,
        response_bytes,
        started_at.elapsed().as_secs_f64() * 1_000.0,
        status.is_client_error() || status.is_server_error(),
    );

    let mut response = Response::new(Body::from(response_body));
    *response.status_mut() = status;
    for (name, value) in &response_headers {
        if should_forward_public_header(name) {
            response.headers_mut().append(name.clone(), value.clone());
        }
    }
    Ok(response)
}

fn should_forward_public_header(name: &HeaderName) -> bool {
    !matches!(
        name.as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
    )
}

fn public_upstream_path(uri: &axum::http::Uri) -> String {
    const PUBLIC_PROXY_PREFIXES: [&str; 2] =
        ["/api/v1/public/app-services/", "/public/app-services/"];
    let path_and_query = uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let Some(service_and_path) = PUBLIC_PROXY_PREFIXES
        .iter()
        .find_map(|prefix| path_and_query.strip_prefix(prefix))
    else {
        return "/".to_owned();
    };
    let suffix = service_and_path.get(36..).unwrap_or_default();
    match suffix {
        "" => "/".to_owned(),
        suffix if suffix.starts_with('?') => format!("/{suffix}"),
        suffix => suffix.to_owned(),
    }
}

fn public_domain_upstream_path(uri: &axum::http::Uri) -> String {
    uri.path_and_query()
        .map(|value| value.as_str().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/".to_owned())
}

fn url_host(host: &str) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    }
}

pub async fn update_auto_deploy(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServiceAutoDeployRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;

    if state.config.database_cluster_provider != PROVIDER_DOCKER {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_DOCKER_REQUIRED",
            message: "App services currently require the Docker provider.",
        });
    }

    let mut transaction = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(advisory_lock_key(project_id))
        .execute(&mut *transaction)
        .await?;
    let existing = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1 AND project_id = $2 FOR UPDATE"
    ))
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;

    if input.enabled && existing.image_source != IMAGE_SOURCE_GITHUB {
        return Err(AppError::BadRequest {
            code: "AUTO_DEPLOY_GITHUB_ONLY",
            message: "Automatic image deploys are available for GitHub Container Registry images only.",
        });
    }
    if input.enabled && existing.status != STATUS_READY {
        return Err(AppError::Conflict {
            code: "APP_SERVICE_NOT_READY",
            message: "The app service must be ready before automatic image deploys can be enabled.",
        });
    }
    if input.enabled && github::docker_credentials(&state, user.id).await?.is_none() {
        return Err(AppError::Conflict {
            code: "GITHUB_CONNECTION_REQUIRED",
            message: "Connect GitHub before enabling automatic image deploys.",
        });
    }

    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "UPDATE project_app_services
         SET auto_deploy_enabled = $1,
             github_connection_user_id = CASE WHEN $1 THEN $2 ELSE github_connection_user_id END,
             auto_deploy_error = NULL,
             updated_at = now()
         WHERE id = $3
         RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(input.enabled)
    .bind(user.id)
    .bind(app_service_id)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let database =
        database_resource_by_id(&state, project_id, service.database_resource_id).await?;
    Ok(Json(app_service_response(
        &service,
        &state.config.app_service_public_host,
        state.config.bind_addr.port(),
        state.config.app_service_public_domain.as_deref(),
        &state.config.app_service_public_scheme,
        database.as_ref(),
        latest_deployment(&state.db, service.id).await?,
    )?))
}

pub async fn metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Query(query): Query<AppServiceMetricsQuery>,
) -> Result<Json<AppServiceMetricsResponse>, AppError> {
    let range = parse_metric_range(query.range.as_deref())?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let target = sqlx::query_as::<_, AppServiceLogsTarget>(
        "SELECT container_name, status FROM project_app_services WHERE id = $1 AND project_id = $2",
    )
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;

    let Some(container_name) = target.container_name else {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_NOT_READY",
            message: "Live metrics become available after the app service is deployed.",
        });
    };
    if target.status != STATUS_READY {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_NOT_READY",
            message: "Live metrics become available after the app service is ready.",
        });
    }

    let runtime_metrics =
        cluster::collect_app_service_runtime_metrics(&state.config, &container_name)
            .await
            .map_err(|error| {
                tracing::warn!(
                    app_service_id = %app_service_id,
                    container_name = %container_name,
                    error = %error,
                    "could not collect app service runtime metrics"
                );
                AppError::ServiceUnavailable {
                    code: "METRICS_UNAVAILABLE",
                    message: "Runtime metrics are temporarily unavailable.",
                }
            })?;
    let public_traffic = public_traffic_metric_values(&state, app_service_id);
    let sample = AppServiceMetricPoint {
        timestamp: unix_timestamp(),
        cpu_percent: runtime_metrics.cpu_percent,
        memory_used_bytes: runtime_metrics.memory_used_bytes,
        memory_limit_bytes: runtime_metrics.memory_limit_bytes,
        volume_used_bytes: runtime_metrics.volume_used_bytes,
        volume_capacity_bytes: runtime_metrics
            .volume_capacity_bytes
            .or(Some(cluster::RESOURCE_VOLUME_LIMIT_BYTES)),
        network_receive_bytes: runtime_metrics.network_receive_bytes,
        network_transmit_bytes: runtime_metrics.network_transmit_bytes,
        disk_read_bytes: runtime_metrics.disk_read_bytes,
        disk_write_bytes: runtime_metrics.disk_write_bytes,
        public_network_receive_bytes: public_traffic.public_network_receive_bytes,
        public_network_transmit_bytes: public_traffic.public_network_transmit_bytes,
        requests: public_traffic.requests,
        response_time_ms: public_traffic.response_time_ms,
        request_error_rate: public_traffic.request_error_rate,
    };
    let sample_timestamp = sample.timestamp;
    persist_app_service_metric_sample(&state, app_service_id, &sample).await?;
    let system_metrics_available = has_system_metrics(&sample);
    let system_metrics_message = if system_metrics_available {
        None
    } else {
        Some(
            "CPU, memory, network, and disk metrics are not available for this app service."
                .to_owned(),
        )
    };
    let from_timestamp = sample_timestamp.saturating_sub(range.seconds);
    let points = load_app_service_metric_history(
        &state,
        app_service_id,
        from_timestamp,
        sample_timestamp,
        range,
    )
    .await?;

    Ok(Json(AppServiceMetricsResponse {
        provider: PROVIDER_DOCKER.to_owned(),
        system_metrics_available,
        system_metrics_message,
        sample_interval_seconds: METRIC_SAMPLE_INTERVAL_SECONDS as u32,
        retention_seconds: METRIC_RETENTION_SECONDS as u32,
        range: range.key.to_owned(),
        from_timestamp,
        to_timestamp: sample_timestamp,
        resolution_seconds: range.bucket_seconds() as u32,
        points,
    }))
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
    Json(input): Json<CreateAppServiceRequest>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;

    if !state.config.app_service_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_PROVISIONING_DISABLED",
            message: "App service provisioning is not enabled for this environment.",
        });
    }
    if state.config.database_cluster_provider != PROVIDER_DOCKER {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_DOCKER_REQUIRED",
            message: "App services currently require the Docker provider.",
        });
    }

    let name = validate_service_name(input.name.as_deref().unwrap_or("App service"))?;
    let image = validate_image(&input.image)?;
    let image_source = validate_image_source(&input.image_source, &image)?;
    let app_port = validate_app_port(input.app_port)?;
    let auto_deploy_enabled =
        image_source == IMAGE_SOURCE_GITHUB && input.auto_deploy.unwrap_or(true);
    let github_connection_user_id = (image_source == IMAGE_SOURCE_GITHUB).then_some(user.id);
    let service = {
        let mut transaction = state.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *transaction)
            .await?;

        let service_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM project_app_services WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        if service_count >= MAX_APP_SERVICES_PER_PROJECT {
            return Err(AppError::Conflict {
                code: "APP_SERVICE_LIMIT_REACHED",
                message: "A project can have up to 6 app services.",
            });
        }

        let service = sqlx::query_as::<_, AppServiceRow>(&format!(
            "INSERT INTO project_app_services (id, project_id, name, image, image_source, app_port, public_subdomain, status, auto_deploy_enabled, github_connection_user_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {APP_SERVICE_COLUMNS}"
        ))
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(&name)
        .bind(&image)
        .bind(&image_source)
        .bind(i32::from(app_port))
        .bind(new_public_subdomain())
        .bind(STATUS_PROVISIONING)
        .bind(auto_deploy_enabled)
        .bind(github_connection_user_id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        service
    };

    let github_credentials = if image_source == IMAGE_SOURCE_GITHUB {
        match github::docker_credentials(&state, user.id).await? {
            Some(credentials) => Some(credentials),
            None => {
                sqlx::query(
                    "UPDATE project_app_services SET status = $1, error_message = $2, updated_at = now() WHERE id = $3",
                )
                .bind(STATUS_ERROR)
                .bind("Connect GitHub before deploying a private image.")
                .bind(service.id)
                .execute(&state.db)
                .await?;
                return Err(AppError::Conflict {
                    code: "GITHUB_CONNECTION_REQUIRED",
                    message: "Connect GitHub before deploying a private GitHub image.",
                });
            }
        }
    } else {
        None
    };

    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO app_service_deployments (id, app_service_id, status, current_step, logs) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(deployment_id)
    .bind(service.id)
    .bind(STATUS_PROVISIONING)
    .bind("Queued")
    .bind("Deployment queued.")
    .execute(&state.db)
    .await?;

    let response = app_service_response(
        &service,
        &state.config.app_service_public_host,
        state.config.bind_addr.port(),
        state.config.app_service_public_domain.as_deref(),
        &state.config.app_service_public_scheme,
        None,
        latest_deployment(&state.db, service.id).await?,
    )?;
    let deployment_state = state.clone();
    tokio::spawn(async move {
        run_app_service_deployment(
            deployment_state,
            deployment_id,
            service.id,
            project_id,
            image,
            app_port,
            github_credentials,
            None,
            None,
            false,
            false,
        )
        .await;
    });

    Ok((StatusCode::ACCEPTED, Json(response)).into_response())
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServiceRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;

    if !state.config.app_service_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_PROVISIONING_DISABLED",
            message: "App service provisioning is not enabled for this environment.",
        });
    }
    if state.config.database_cluster_provider != PROVIDER_DOCKER {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_DOCKER_REQUIRED",
            message: "App services currently require the Docker provider.",
        });
    }

    let app_port = validate_app_port(Some(input.app_port))?;

    let (service, database_resource_id, previous_container_name) = {
        let mut transaction = state.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *transaction)
            .await?;

        let existing = sqlx::query_as::<_, AppServiceRow>(&format!(
            "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1 AND project_id = $2 FOR UPDATE"
        ))
        .bind(app_service_id)
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound {
            code: "APP_SERVICE_NOT_FOUND",
            message: "Deploy an app service before changing its container port.",
        })?;

        if existing.status == STATUS_PROVISIONING {
            return Err(AppError::Conflict {
                code: "APP_SERVICE_PROVISIONING",
                message: "This app service is already being deployed.",
            });
        }

        if existing.status == STATUS_READY && existing.app_port == i32::from(app_port) {
            transaction.commit().await?;
            let database =
                database_resource_by_id(&state, project_id, existing.database_resource_id).await?;
            return Ok(Json(app_service_response(
                &existing,
                &state.config.app_service_public_host,
                state.config.bind_addr.port(),
                state.config.app_service_public_domain.as_deref(),
                &state.config.app_service_public_scheme,
                database.as_ref(),
                latest_deployment(&state.db, existing.id).await?,
            )?));
        }

        let service = sqlx::query_as::<_, AppServiceRow>(&format!(
            "UPDATE project_app_services SET app_port = $1, host = NULL, port = NULL, status = $2, error_message = NULL, updated_at = now() WHERE id = $3 RETURNING {APP_SERVICE_COLUMNS}"
        ))
        .bind(i32::from(app_port))
        .bind(STATUS_PROVISIONING)
        .bind(existing.id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        (
            service,
            existing.database_resource_id,
            existing.container_name,
        )
    };

    let database = database_resource_by_id(&state, project_id, database_resource_id).await?;

    let github_credentials = if service.image_source == IMAGE_SOURCE_GITHUB {
        match github::docker_credentials(&state, user.id).await? {
            Some(credentials) => Some(credentials),
            None => {
                sqlx::query(
                    "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = $3, updated_at = now() WHERE id = $4",
                )
                .bind(STATUS_ERROR)
                .bind("Connect GitHub before deploying a private image.")
                .bind(database_resource_id)
                .bind(service.id)
                .execute(&state.db)
                .await?;
                return Err(AppError::Conflict {
                    code: "GITHUB_CONNECTION_REQUIRED",
                    message: "Connect GitHub before deploying a private GitHub image.",
                });
            }
        }
    } else {
        None
    };

    let provisioned = provision_docker(
        &state,
        project_id,
        service.id,
        &service.image,
        app_port,
        github_credentials.as_ref(),
        database.as_ref(),
        true,
        previous_container_name.as_deref(),
        None,
    )
    .await;
    let provisioned = match provisioned {
        Ok(provisioned) => provisioned,
        Err(error) => {
            tracing::error!(
                project_id = %project_id,
                service_id = %service.id,
                error = %error,
                "app service port update failed"
            );
            sqlx::query(
                "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = $3, updated_at = now() WHERE id = $4",
            )
            .bind(STATUS_ERROR)
            .bind(PROVISIONING_ERROR_MESSAGE)
            .bind(database_resource_id)
            .bind(service.id)
            .execute(&state.db)
            .await?;
            return Err(AppError::ServiceUnavailable {
                code: "APP_SERVICE_PROVISIONING_FAILED",
                message: PROVISIONING_ERROR_MESSAGE,
            });
        }
    };

    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "UPDATE project_app_services SET status = $1, host = $2, port = $3, app_port = $4, container_name = $5, database_resource_id = $6, deployed_image_digest = $7, error_message = NULL, auto_deploy_error = NULL, updated_at = now() WHERE id = $8 RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(STATUS_READY)
    .bind(&provisioned.host)
    .bind(i32::from(provisioned.port))
    .bind(i32::from(provisioned.app_port))
    .bind(&provisioned.container_name)
    .bind(database_resource_id)
    .bind(&provisioned.image_digest)
    .bind(service.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(app_service_response(
        &service,
        &state.config.app_service_public_host,
        state.config.bind_addr.port(),
        state.config.app_service_public_domain.as_deref(),
        &state.config.app_service_public_scheme,
        database.as_ref(),
        latest_deployment(&state.db, service.id).await?,
    )?))
}

pub async fn update_database_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServiceDatabaseRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;

    if !state.config.app_service_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_PROVISIONING_DISABLED",
            message: "App service provisioning is not enabled for this environment.",
        });
    }
    if state.config.database_cluster_provider != PROVIDER_DOCKER {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_DOCKER_REQUIRED",
            message: "App services currently require the Docker provider.",
        });
    }

    let (service, previous_database_id, previous_container_name, database) = {
        let mut transaction = state.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *transaction)
            .await?;

        let existing = sqlx::query_as::<_, AppServiceRow>(&format!(
            "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1 AND project_id = $2 FOR UPDATE"
        ))
        .bind(app_service_id)
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::NotFound {
            code: "APP_SERVICE_NOT_FOUND",
            message: "Deploy an app service before changing its database connection.",
        })?;

        if existing.status == STATUS_PROVISIONING {
            return Err(AppError::Conflict {
                code: "APP_SERVICE_PROVISIONING",
                message: "This app service is already being deployed.",
            });
        }

        let database = match input.database_resource_id {
            Some(database_id) => Some(
                sqlx::query_as::<_, DatabaseResourceRow>(&format!(
                    "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE id = $1 AND project_id = $2 AND status = 'ready' AND cluster_provider = $3"
                ))
                .bind(database_id)
                .bind(project_id)
                .bind(PROVIDER_DOCKER)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(AppError::NotFound {
                    code: "DATABASE_RESOURCE_NOT_FOUND",
                    message: "The selected PostgreSQL database is not ready in this project.",
                })?,
            ),
            None => None,
        };

        if existing.database_resource_id == input.database_resource_id {
            transaction.commit().await?;
            return Ok(Json(app_service_response(
                &existing,
                &state.config.app_service_public_host,
                state.config.bind_addr.port(),
                state.config.app_service_public_domain.as_deref(),
                &state.config.app_service_public_scheme,
                database.as_ref(),
                latest_deployment(&state.db, existing.id).await?,
            )?));
        }

        let service = sqlx::query_as::<_, AppServiceRow>(&format!(
            "UPDATE project_app_services SET host = NULL, port = NULL, status = $1, error_message = NULL, updated_at = now() WHERE id = $2 RETURNING {APP_SERVICE_COLUMNS}"
        ))
        .bind(STATUS_PROVISIONING)
        .bind(existing.id)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        (
            service,
            existing.database_resource_id,
            existing.container_name,
            database,
        )
    };

    let github_credentials = if service.image_source == IMAGE_SOURCE_GITHUB {
        match github::docker_credentials(&state, user.id).await? {
            Some(credentials) => Some(credentials),
            None => {
                sqlx::query(
                    "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = $3, updated_at = now() WHERE id = $4",
                )
                .bind(STATUS_ERROR)
                .bind("Connect GitHub before deploying a private image.")
                .bind(previous_database_id)
                .bind(service.id)
                .execute(&state.db)
                .await?;
                return Err(AppError::Conflict {
                    code: "GITHUB_CONNECTION_REQUIRED",
                    message: "Connect GitHub before deploying a private GitHub image.",
                });
            }
        }
    } else {
        None
    };

    let provisioned = provision_docker(
        &state,
        project_id,
        service.id,
        &service.image,
        u16::try_from(service.app_port)
            .map_err(|_| AppError::internal("invalid app service container port"))?,
        github_credentials.as_ref(),
        database.as_ref(),
        true,
        previous_container_name.as_deref(),
        None,
    )
    .await;
    let provisioned = match provisioned {
        Ok(provisioned) => provisioned,
        Err(error) => {
            tracing::error!(
                project_id = %project_id,
                service_id = %service.id,
                error = %error,
                "app service database connection update failed"
            );
            sqlx::query(
                "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = $3, updated_at = now() WHERE id = $4",
            )
            .bind(STATUS_ERROR)
            .bind(PROVISIONING_ERROR_MESSAGE)
            .bind(previous_database_id)
            .bind(service.id)
            .execute(&state.db)
            .await?;
            return Err(AppError::ServiceUnavailable {
                code: "APP_SERVICE_PROVISIONING_FAILED",
                message: PROVISIONING_ERROR_MESSAGE,
            });
        }
    };

    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "UPDATE project_app_services SET status = $1, host = $2, port = $3, app_port = $4, container_name = $5, database_resource_id = $6, deployed_image_digest = $7, error_message = NULL, auto_deploy_error = NULL, updated_at = now() WHERE id = $8 RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(STATUS_READY)
    .bind(&provisioned.host)
    .bind(i32::from(provisioned.port))
    .bind(i32::from(provisioned.app_port))
    .bind(&provisioned.container_name)
    .bind(input.database_resource_id)
    .bind(&provisioned.image_digest)
    .bind(service.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(app_service_response(
        &service,
        &state.config.app_service_public_host,
        state.config.bind_addr.port(),
        state.config.app_service_public_domain.as_deref(),
        &state.config.app_service_public_scheme,
        database.as_ref(),
        latest_deployment(&state.db, service.id).await?,
    )?))
}

fn validate_service_name(value: &str) -> Result<String, AppError> {
    let name = value.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        let mut fields = BTreeMap::new();
        fields.insert(
            "name".to_owned(),
            "Enter an app service name between 1 and 80 characters.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(name)
}

fn validate_image(value: &str) -> Result<String, AppError> {
    let image = value.trim().to_owned();
    let valid = !image.is_empty()
        && image.len() <= 255
        && !image.starts_with('/')
        && !image.ends_with('/')
        && !image.contains("://")
        && image.chars().all(|character| {
            character.is_ascii_alphanumeric()
                || matches!(character, '/' | '.' | '_' | ':' | '-' | '@')
        });
    if !valid {
        let mut fields = BTreeMap::new();
        fields.insert(
            "image".to_owned(),
            "Paste a valid Docker image reference, for example nginx:alpine.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(image)
}

fn validate_image_source(source: &str, image: &str) -> Result<String, AppError> {
    let normalized = source.trim().to_ascii_lowercase();
    if normalized == IMAGE_SOURCE_GITHUB
        && !image
            .get(.."ghcr.io/".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ghcr.io/"))
    {
        let mut fields = BTreeMap::new();
        fields.insert(
            "image".to_owned(),
            "Private GitHub images must use the ghcr.io registry.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    if normalized != IMAGE_SOURCE_PUBLIC && normalized != IMAGE_SOURCE_GITHUB {
        let mut fields = BTreeMap::new();
        fields.insert(
            "imageSource".to_owned(),
            "Choose a public image or a private GitHub image.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(normalized)
}

fn validate_app_port(value: Option<u32>) -> Result<u16, AppError> {
    let port = value.unwrap_or(u32::from(DEFAULT_APP_PORT));
    if !(1..=u32::from(u16::MAX)).contains(&port) {
        let mut fields = BTreeMap::new();
        fields.insert(
            "appPort".to_owned(),
            "Use a container port between 1 and 65535.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(port as u16)
}

fn app_service_response(
    service: &AppServiceRow,
    public_host: &str,
    api_port: u16,
    public_domain: Option<&str>,
    public_scheme: &str,
    database: Option<&DatabaseResourceRow>,
    deployment: Option<AppServiceDeploymentResponse>,
) -> Result<AppServiceResponse, AppError> {
    let app_port = u16::try_from(service.app_port)
        .map_err(|_| AppError::internal("invalid app service container port"))?;
    let port = service
        .port
        .map(u16::try_from)
        .transpose()
        .map_err(|_| AppError::internal("invalid app service published port"))?;
    let host = service
        .host
        .clone()
        .or_else(|| (service.status == STATUS_READY).then(|| public_host.to_owned()));
    let public_domain = public_domain.map(|domain| {
        format!(
            "{}.{}",
            service.public_subdomain,
            domain.trim_end_matches('.')
        )
    });
    let service_url = if service.status == STATUS_READY && service.container_name.is_some() {
        public_domain
            .as_deref()
            .map(|domain| format!("{public_scheme}://{domain}"))
            .or_else(|| match (host.as_deref(), port) {
                (Some(_), Some(_)) => Some(format!(
                    "http://{}:{}/api/v1/public/app-services/{}",
                    url_host(public_host),
                    api_port,
                    service.id
                )),
                _ => None,
            })
    } else {
        None
    };

    Ok(AppServiceResponse {
        id: service.id,
        name: service.name.clone(),
        resource_type: "app".to_owned(),
        status: service.status.clone(),
        image: service.image.clone(),
        image_source: service.image_source.clone(),
        app_port,
        host,
        port,
        service_url,
        public_domain,
        container_name: service.container_name.clone(),
        error_message: service.error_message.clone(),
        auto_deploy_enabled: service.auto_deploy_enabled,
        deployed_image_digest: service.deployed_image_digest.clone(),
        auto_deploy_checked_at: service
            .auto_deploy_checked_at
            .and_then(|value| value.format(&Rfc3339).ok()),
        auto_deploy_error: service.auto_deploy_error.clone(),
        database_connection: service
            .database_resource_id
            .and_then(|resource_id| database.filter(|database| database.id == resource_id))
            .map(|database| database_connection_response(service.project_id, database)),
        deployment,
    })
}

async fn latest_deployment(
    db: &sqlx::PgPool,
    app_service_id: Uuid,
) -> std::result::Result<Option<AppServiceDeploymentResponse>, sqlx::Error> {
    sqlx::query_as::<_, AppServiceDeploymentRow>(
        "SELECT id, status, current_step, logs, error_message, updated_at FROM app_service_deployments WHERE app_service_id = $1 ORDER BY started_at DESC LIMIT 1",
    )
    .bind(app_service_id)
    .fetch_optional(db)
    .await
    .map(|deployment| deployment.map(|deployment| deployment_response(&deployment)))
}

fn deployment_response(deployment: &AppServiceDeploymentRow) -> AppServiceDeploymentResponse {
    AppServiceDeploymentResponse {
        id: deployment.id,
        status: deployment.status.clone(),
        current_step: deployment.current_step.clone(),
        logs: deployment.logs.lines().map(str::to_owned).collect(),
        error_message: deployment.error_message.clone(),
    }
}

fn database_connection_response(
    project_id: Uuid,
    database: &DatabaseResourceRow,
) -> AppServiceDatabaseConnectionResponse {
    AppServiceDatabaseConnectionResponse {
        resource_id: database.id,
        name: database.name.clone(),
        database_name: database.database_name.clone(),
        username: database.role_name.clone(),
        network_name: cluster::project_network_name(project_id),
        host: cluster::PROJECT_NETWORK_POSTGRES_ALIAS.to_owned(),
        port: 5432,
        environment_variables: vec![
            "DATABASE_URL".to_owned(),
            "PGHOST".to_owned(),
            "PGPORT".to_owned(),
            "PGDATABASE".to_owned(),
            "PGUSER".to_owned(),
            "PGPASSWORD".to_owned(),
        ],
    }
}

#[derive(Debug, Clone)]
struct DeploymentLogger {
    db: sqlx::PgPool,
    deployment_id: Uuid,
}

impl DeploymentLogger {
    async fn append(&self, step: &str, message: &str) -> Result<()> {
        let message = message.trim();
        if message.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE app_service_deployments SET current_step = $1, logs = right(CASE WHEN logs = '' THEN $2 ELSE logs || E'\\n' || $2 END, 200000), updated_at = now() WHERE id = $3",
        )
        .bind(step)
        .bind(message)
        .bind(self.deployment_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }

    async fn finish(&self, status: &str, step: &str, error_message: Option<&str>) -> Result<()> {
        sqlx::query(
            "UPDATE app_service_deployments SET status = $1, current_step = $2, error_message = $3, finished_at = now(), updated_at = now() WHERE id = $4",
        )
        .bind(status)
        .bind(step)
        .bind(error_message)
        .bind(self.deployment_id)
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

async fn log_deployment(
    logger: Option<&DeploymentLogger>,
    step: &str,
    message: &str,
) -> Result<()> {
    if let Some(logger) = logger {
        logger.append(step, message).await?;
    }
    Ok(())
}

async fn run_app_service_deployment(
    state: AppState,
    deployment_id: Uuid,
    service_id: Uuid,
    project_id: Uuid,
    image: String,
    app_port: u16,
    github_credentials: Option<GithubDockerCredentials>,
    database: Option<DatabaseResourceRow>,
    database_resource_id: Option<Uuid>,
    honor_requested_port: bool,
    automatic: bool,
) {
    let logger = DeploymentLogger {
        db: state.db.clone(),
        deployment_id,
    };
    let result = async {
        logger
            .append("Start", "Deployment worker started.")
            .await?;
        let provisioned = provision_docker(
            &state,
            project_id,
            service_id,
            &image,
            app_port,
            github_credentials.as_ref(),
            database.as_ref(),
            honor_requested_port,
            None,
            Some(&logger),
        )
        .await?;
        sqlx::query(
            "UPDATE project_app_services SET status = $1, host = $2, port = $3, app_port = $4, container_name = $5, database_resource_id = $6, deployed_image_digest = $7, error_message = NULL, auto_deploy_error = NULL, auto_deploy_checked_at = CASE WHEN $8 THEN now() ELSE auto_deploy_checked_at END, updated_at = now() WHERE id = $9",
        )
        .bind(STATUS_READY)
        .bind(&provisioned.host)
        .bind(i32::from(provisioned.port))
        .bind(i32::from(provisioned.app_port))
        .bind(&provisioned.container_name)
        .bind(database_resource_id)
        .bind(&provisioned.image_digest)
        .bind(automatic)
        .bind(service_id)
        .execute(&state.db)
        .await?;
        logger
            .append(
                "Complete",
                &format!("Container {} is ready for traffic.", provisioned.container_name),
            )
            .await?;
        logger.finish(STATUS_READY, "Complete", None).await?;
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(error) = result {
        tracing::error!(
            project_id = %project_id,
            service_id = %service_id,
            error = %error,
            "app service provisioning failed"
        );
        let detail = format!("Deployment failed: {error:#}");
        for line in detail.lines() {
            if let Err(log_error) = logger.append("Failed", line).await {
                tracing::warn!(error = %log_error, "could not persist app service deployment error log");
            }
        }
        if let Err(update_error) = sqlx::query(
            "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = $3, auto_deploy_error = CASE WHEN $4 THEN $2 ELSE auto_deploy_error END, auto_deploy_checked_at = CASE WHEN $4 THEN now() ELSE auto_deploy_checked_at END, updated_at = now() WHERE id = $5",
        )
        .bind(STATUS_ERROR)
        .bind(PROVISIONING_ERROR_MESSAGE)
        .bind(database_resource_id)
        .bind(automatic)
        .bind(service_id)
        .execute(&state.db)
        .await
        {
            tracing::error!(error = %update_error, "could not mark app service deployment as failed");
        }
        if let Err(finish_error) = logger
            .finish(STATUS_ERROR, "Failed", Some(PROVISIONING_ERROR_MESSAGE))
            .await
        {
            tracing::error!(error = %finish_error, "could not finish app service deployment record");
        }
    }
}

const AUTO_DEPLOY_CHECK_ERROR_MESSAGE: &str =
    "Automatic image update check failed. Verify the GitHub connection and image tag.";

pub fn spawn_auto_deployer(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(AUTO_DEPLOY_INTERVAL_SECONDS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            interval.tick().await;
            if let Err(error) = poll_auto_deployments(&state).await {
                tracing::warn!(error = %error, "could not enumerate automatic app service deployments");
            }
        }
    })
}

async fn poll_auto_deployments(state: &AppState) -> std::result::Result<(), sqlx::Error> {
    let candidates = sqlx::query_as::<_, AutoDeployCandidate>(
        "SELECT id, image, container_name, github_connection_user_id
         FROM project_app_services
         WHERE auto_deploy_enabled = TRUE
           AND image_source = $1
           AND status = $2
         ORDER BY auto_deploy_checked_at NULLS FIRST, id ASC",
    )
    .bind(IMAGE_SOURCE_GITHUB)
    .bind(STATUS_READY)
    .fetch_all(&state.db)
    .await?;

    // Registry login is process-global in Docker. Check services serially so
    // two users' credentials cannot race through `docker login`/`logout`.
    for candidate in candidates {
        if let Err(error) = check_auto_deploy_candidate(state, candidate.clone()).await {
            tracing::warn!(
                app_service_id = %candidate.id,
                error = %error,
                "automatic app service image check failed"
            );
            if let Err(update_error) = record_auto_deploy_error(state, candidate.id).await {
                tracing::warn!(
                    app_service_id = %candidate.id,
                    error = %update_error,
                    "could not persist automatic app service image check error"
                );
            }
        }
    }
    Ok(())
}

async fn check_auto_deploy_candidate(
    state: &AppState,
    candidate: AutoDeployCandidate,
) -> Result<()> {
    let user_id = candidate
        .github_connection_user_id
        .context("the automatic deploy has no GitHub connection owner")?;
    let credentials = github::docker_credentials(state, user_id)
        .await
        .map_err(|error| anyhow::anyhow!("could not load GitHub credentials: {error:?}"))?
        .context("the GitHub connection is no longer available")?;
    let container_name = candidate
        .container_name
        .as_deref()
        .context("the ready app service has no container")?;
    let running_digest = docker_container_image_digest(state, container_name)
        .await?
        .context("Docker did not return the running container image digest")?;

    pull_github_image(state, &credentials, &candidate.image, None).await?;
    let pulled_digest = docker_image_digest(state, &candidate.image, None)
        .await?
        .context("Docker did not return the pulled image digest")?;
    // The running container is authoritative. The stored digest is metadata
    // and can be stale if an operator redeployed outside of this API.
    let baseline_digest = running_digest;

    if baseline_digest == pulled_digest {
        record_auto_deploy_check(state, candidate.id, &pulled_digest).await?;
        return Ok(());
    }

    if let Some(queued) =
        queue_auto_deployment(state, candidate.id, Some(&baseline_digest), &pulled_digest).await?
    {
        let credentials = Some(credentials);
        let state = state.clone();
        tokio::spawn(async move {
            run_app_service_deployment(
                state,
                queued.deployment_id,
                queued.service_id,
                queued.project_id,
                queued.image,
                queued.app_port,
                credentials,
                queued.database,
                queued.database_resource_id,
                false,
                true,
            )
            .await;
        });
    }
    Ok(())
}

async fn record_auto_deploy_check(
    state: &AppState,
    service_id: Uuid,
    digest: &str,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE project_app_services
         SET deployed_image_digest = $1,
             auto_deploy_checked_at = now(),
             auto_deploy_error = NULL,
             updated_at = now()
         WHERE id = $2 AND auto_deploy_enabled = TRUE",
    )
    .bind(digest)
    .bind(service_id)
    .execute(&state.db)
    .await
    .map(|_| ())
}

async fn record_auto_deploy_error(
    state: &AppState,
    service_id: Uuid,
) -> std::result::Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE project_app_services
         SET auto_deploy_checked_at = now(), auto_deploy_error = $1, updated_at = now()
         WHERE id = $2 AND auto_deploy_enabled = TRUE",
    )
    .bind(AUTO_DEPLOY_CHECK_ERROR_MESSAGE)
    .bind(service_id)
    .execute(&state.db)
    .await
    .map(|_| ())
}

async fn queue_auto_deployment(
    state: &AppState,
    service_id: Uuid,
    expected_digest: Option<&str>,
    pulled_digest: &str,
) -> Result<Option<QueuedAutoDeployment>> {
    let mut transaction = state.db.begin().await?;
    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1 FOR UPDATE"
    ))
    .bind(service_id)
    .fetch_optional(&mut *transaction)
    .await?
    .context("the app service disappeared during automatic deployment")?;

    if service.status != STATUS_READY
        || !service.auto_deploy_enabled
        || service.image_source != IMAGE_SOURCE_GITHUB
    {
        transaction.commit().await?;
        return Ok(None);
    }

    let current_digest = expected_digest.or(service.deployed_image_digest.as_deref());
    if current_digest == Some(pulled_digest) {
        sqlx::query(
            "UPDATE project_app_services
             SET deployed_image_digest = $1,
                 auto_deploy_checked_at = now(),
                 auto_deploy_error = NULL,
                 updated_at = now()
             WHERE id = $2",
        )
        .bind(pulled_digest)
        .bind(service.id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(None);
    }

    let database = match service.database_resource_id {
        Some(resource_id) => Some(
            sqlx::query_as::<_, DatabaseResourceRow>(&format!(
                "SELECT {DATABASE_RESOURCE_COLUMNS}
                 FROM project_postgres_databases
                 WHERE id = $1 AND project_id = $2 AND status = 'ready' AND cluster_provider = $3"
            ))
            .bind(resource_id)
            .bind(service.project_id)
            .bind(PROVIDER_DOCKER)
            .fetch_optional(&mut *transaction)
            .await?
            .context("the attached database is no longer ready")?,
        ),
        None => None,
    };
    let app_port = u16::try_from(service.app_port)
        .map_err(|_| anyhow::anyhow!("invalid app service container port"))?;
    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "UPDATE project_app_services
         SET status = $1, host = NULL, port = NULL, error_message = NULL,
             auto_deploy_checked_at = now(), auto_deploy_error = NULL, updated_at = now()
         WHERE id = $2",
    )
    .bind(STATUS_PROVISIONING)
    .bind(service.id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO app_service_deployments (id, app_service_id, status, current_step, logs)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(deployment_id)
    .bind(service.id)
    .bind(STATUS_PROVISIONING)
    .bind("Queued")
    .bind(format!(
        "Automatic deploy queued for image digest {}.",
        short_image_digest(pulled_digest)
    ))
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Some(QueuedAutoDeployment {
        deployment_id,
        service_id: service.id,
        project_id: service.project_id,
        image: service.image,
        app_port,
        database_resource_id: service.database_resource_id,
        database,
    }))
}

fn short_image_digest(digest: &str) -> String {
    if digest.len() <= 19 {
        return digest.to_owned();
    }
    format!("{}…{}", &digest[..15], &digest[digest.len() - 4..])
}

async fn persist_app_service_metric_sample(
    state: &AppState,
    app_service_id: Uuid,
    sample: &AppServiceMetricPoint,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO app_service_metric_samples (
            app_service_id, sampled_at, cpu_percent, memory_used_bytes, memory_limit_bytes,
            volume_used_bytes, volume_capacity_bytes, network_receive_bytes,
            network_transmit_bytes, disk_read_bytes, disk_write_bytes,
            public_network_receive_bytes, public_network_transmit_bytes,
            requests, response_time_ms, request_error_rate
         ) VALUES (
            $1, to_timestamp($2::double precision), $3, $4, $5, $6, $7, $8, $9, $10, $11,
            $12, $13, $14, $15, $16
         )
         ON CONFLICT (app_service_id, sampled_at) DO UPDATE SET
            cpu_percent = EXCLUDED.cpu_percent,
            memory_used_bytes = EXCLUDED.memory_used_bytes,
            memory_limit_bytes = EXCLUDED.memory_limit_bytes,
            volume_used_bytes = EXCLUDED.volume_used_bytes,
            volume_capacity_bytes = EXCLUDED.volume_capacity_bytes,
            network_receive_bytes = EXCLUDED.network_receive_bytes,
            network_transmit_bytes = EXCLUDED.network_transmit_bytes,
            disk_read_bytes = EXCLUDED.disk_read_bytes,
            disk_write_bytes = EXCLUDED.disk_write_bytes,
            public_network_receive_bytes = EXCLUDED.public_network_receive_bytes,
            public_network_transmit_bytes = EXCLUDED.public_network_transmit_bytes,
            requests = EXCLUDED.requests,
            response_time_ms = EXCLUDED.response_time_ms,
            request_error_rate = EXCLUDED.request_error_rate",
    )
    .bind(app_service_id)
    .bind(sample.timestamp)
    .bind(sample.cpu_percent)
    .bind(sample.memory_used_bytes)
    .bind(sample.memory_limit_bytes)
    .bind(sample.volume_used_bytes)
    .bind(sample.volume_capacity_bytes)
    .bind(sample.network_receive_bytes)
    .bind(sample.network_transmit_bytes)
    .bind(sample.disk_read_bytes)
    .bind(sample.disk_write_bytes)
    .bind(sample.public_network_receive_bytes)
    .bind(sample.public_network_transmit_bytes)
    .bind(sample.requests)
    .bind(sample.response_time_ms)
    .bind(sample.request_error_rate)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "DELETE FROM app_service_metric_samples
         WHERE app_service_id = $1
           AND sampled_at < now() - INTERVAL '30 days'",
    )
    .bind(app_service_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(())
}

async fn load_app_service_metric_history(
    state: &AppState,
    app_service_id: Uuid,
    from_timestamp: i64,
    to_timestamp: i64,
    range: crate::metrics::MetricRange,
) -> Result<Vec<AppServiceMetricPoint>, AppError> {
    let rows = sqlx::query_as::<_, AppServiceMetricHistoryRow>(
        "WITH bucketed AS (
            SELECT
                EXTRACT(EPOCH FROM sampled_at)::bigint AS sample_timestamp,
                cpu_percent,
                memory_used_bytes,
                memory_limit_bytes,
                volume_used_bytes,
                volume_capacity_bytes,
                network_receive_bytes,
                network_transmit_bytes,
                disk_read_bytes,
                disk_write_bytes,
                public_network_receive_bytes,
                public_network_transmit_bytes,
                requests,
                response_time_ms,
                request_error_rate,
                ROW_NUMBER() OVER (
                    PARTITION BY FLOOR(
                        EXTRACT(EPOCH FROM sampled_at) / $4::double precision
                    )
                    ORDER BY sampled_at DESC
                ) AS sample_rank
            FROM app_service_metric_samples
            WHERE app_service_id = $1
              AND sampled_at >= to_timestamp($2::double precision)
              AND sampled_at <= to_timestamp($3::double precision)
        )
        SELECT sample_timestamp, cpu_percent, memory_used_bytes, memory_limit_bytes,
               volume_used_bytes, volume_capacity_bytes, network_receive_bytes,
               network_transmit_bytes, disk_read_bytes, disk_write_bytes,
               public_network_receive_bytes, public_network_transmit_bytes,
               requests, response_time_ms, request_error_rate
        FROM bucketed
        WHERE sample_rank = 1
        ORDER BY sample_timestamp ASC",
    )
    .bind(app_service_id)
    .bind(from_timestamp)
    .bind(to_timestamp)
    .bind(range.bucket_seconds())
    .fetch_all(&state.db)
    .await
    .map_err(AppError::from)?;

    Ok(downsample_metric_points(
        rows.into_iter()
            .map(|row| AppServiceMetricPoint {
                timestamp: row.sample_timestamp,
                cpu_percent: row.cpu_percent,
                memory_used_bytes: row.memory_used_bytes,
                memory_limit_bytes: row.memory_limit_bytes,
                volume_used_bytes: row.volume_used_bytes,
                volume_capacity_bytes: row.volume_capacity_bytes,
                network_receive_bytes: row.network_receive_bytes,
                network_transmit_bytes: row.network_transmit_bytes,
                disk_read_bytes: row.disk_read_bytes,
                disk_write_bytes: row.disk_write_bytes,
                public_network_receive_bytes: row.public_network_receive_bytes,
                public_network_transmit_bytes: row.public_network_transmit_bytes,
                requests: row.requests,
                response_time_ms: row.response_time_ms,
                request_error_rate: row.request_error_rate,
            })
            .collect(),
        MAX_METRIC_RESPONSE_POINTS,
    ))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AppServiceMetricSamplerRow {
    id: Uuid,
    container_name: String,
}

pub(crate) async fn sample_ready_app_services(
    state: &AppState,
) -> std::result::Result<(), sqlx::Error> {
    let services = sqlx::query_as::<_, AppServiceMetricSamplerRow>(
        "SELECT id, container_name
         FROM project_app_services
         WHERE status = $1 AND container_name IS NOT NULL",
    )
    .bind(STATUS_READY)
    .fetch_all(&state.db)
    .await?;

    let sample_concurrency = metric_sample_concurrency(state.config.database_max_connections);
    let results = stream::iter(services)
        .map(|service| sample_app_service(state, service))
        .buffer_unordered(sample_concurrency)
        .collect::<Vec<_>>()
        .await;
    for result in results {
        if let Err(error) = result {
            tracing::warn!(error = ?error, "could not persist app service metric sample");
        }
    }
    Ok(())
}

async fn sample_app_service(
    state: &AppState,
    service: AppServiceMetricSamplerRow,
) -> Result<(), AppError> {
    let runtime_metrics =
        cluster::collect_app_service_runtime_metrics(&state.config, &service.container_name)
            .await
            .map_err(|error| {
                tracing::warn!(
                    app_service_id = %service.id,
                    container_name = %service.container_name,
                    error = %error,
                    "could not collect app service runtime metrics"
                );
                AppError::ServiceUnavailable {
                    code: "METRICS_UNAVAILABLE",
                    message: "Runtime metrics are temporarily unavailable.",
                }
            })?;
    let public_traffic = public_traffic_metric_values(state, service.id);
    persist_app_service_metric_sample(
        state,
        service.id,
        &AppServiceMetricPoint {
            timestamp: unix_timestamp(),
            cpu_percent: runtime_metrics.cpu_percent,
            memory_used_bytes: runtime_metrics.memory_used_bytes,
            memory_limit_bytes: runtime_metrics.memory_limit_bytes,
            volume_used_bytes: runtime_metrics.volume_used_bytes,
            volume_capacity_bytes: runtime_metrics
                .volume_capacity_bytes
                .or(Some(cluster::RESOURCE_VOLUME_LIMIT_BYTES)),
            network_receive_bytes: runtime_metrics.network_receive_bytes,
            network_transmit_bytes: runtime_metrics.network_transmit_bytes,
            disk_read_bytes: runtime_metrics.disk_read_bytes,
            disk_write_bytes: runtime_metrics.disk_write_bytes,
            public_network_receive_bytes: public_traffic.public_network_receive_bytes,
            public_network_transmit_bytes: public_traffic.public_network_transmit_bytes,
            requests: public_traffic.requests,
            response_time_ms: public_traffic.response_time_ms,
            request_error_rate: public_traffic.request_error_rate,
        },
    )
    .await
}

async fn ready_database_resources(
    state: &AppState,
    project_id: Uuid,
) -> std::result::Result<Vec<DatabaseResourceRow>, sqlx::Error> {
    sqlx::query_as::<_, DatabaseResourceRow>(&format!(
        "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE project_id = $1 AND status = 'ready' AND cluster_provider = $2"
    ))
    .bind(project_id)
    .bind(PROVIDER_DOCKER)
    .fetch_all(&state.db)
    .await
}

async fn database_resource_by_id(
    state: &AppState,
    project_id: Uuid,
    resource_id: Option<Uuid>,
) -> std::result::Result<Option<DatabaseResourceRow>, sqlx::Error> {
    let Some(resource_id) = resource_id else {
        return Ok(None);
    };
    sqlx::query_as::<_, DatabaseResourceRow>(&format!(
        "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE id = $1 AND project_id = $2 AND status = 'ready' AND cluster_provider = $3"
    ))
    .bind(resource_id)
    .bind(project_id)
    .bind(PROVIDER_DOCKER)
    .fetch_optional(&state.db)
    .await
}

fn database_environment(
    encryption_key: &[u8; 32],
    database: Option<&DatabaseResourceRow>,
) -> Result<Vec<String>> {
    let Some(database) = database else {
        return Ok(Vec::new());
    };
    let password = security::decrypt_secret(&database.password_ciphertext, encryption_key)
        .map_err(|_| anyhow::anyhow!("could not decrypt the PostgreSQL credentials"))?;
    let database_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        database.role_name,
        password,
        cluster::PROJECT_NETWORK_POSTGRES_ALIAS,
        5432,
        database.database_name,
    );
    Ok(vec![
        format!("DATABASE_URL={database_url}"),
        format!("PGHOST={}", cluster::PROJECT_NETWORK_POSTGRES_ALIAS),
        "PGPORT=5432".to_owned(),
        format!("PGDATABASE={}", database.database_name),
        format!("PGUSER={}", database.role_name),
        format!("PGPASSWORD={password}"),
    ])
}

#[derive(Debug)]
struct ProvisionedAppService {
    host: String,
    port: u16,
    app_port: u16,
    container_name: String,
    image_digest: Option<String>,
}

fn parse_exposed_ports(output: &str) -> Vec<u16> {
    let mut ports: Vec<u16> = output
        .split(|character: char| !character.is_ascii_digit())
        .filter_map(|token| {
            if token.is_empty() {
                return None;
            }
            let port = token.parse::<u16>().ok()?;
            (port > 0).then_some(port)
        })
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
}

fn resolve_container_port(requested: u16, exposed: &[u16]) -> u16 {
    if exposed.is_empty() || exposed.contains(&requested) {
        return requested;
    }
    if exposed.contains(&80) {
        return 80;
    }
    if exposed.contains(&8080) {
        return 8080;
    }
    exposed[0]
}

async fn provision_docker(
    state: &AppState,
    project_id: Uuid,
    service_id: Uuid,
    image: &str,
    app_port: u16,
    github_credentials: Option<&GithubDockerCredentials>,
    database: Option<&DatabaseResourceRow>,
    honor_requested_port: bool,
    previous_container_name: Option<&str>,
    logger: Option<&DeploymentLogger>,
) -> Result<ProvisionedAppService> {
    log_deployment(logger, "Prepare", "Preparing the Docker deployment.").await?;
    let database_environment =
        database_environment(&state.config.database_credentials_encryption_key, database)?;
    log_deployment(logger, "Network", "Ensuring the project network exists.").await?;
    let network_name = cluster::ensure_project_network(&state.config, project_id).await?;
    log_deployment(logger, "Network", "Project network is ready.").await?;

    if let Some(credentials) = github_credentials {
        if !image
            .get(.."ghcr.io/".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ghcr.io/"))
        {
            bail!("private app images must be hosted on ghcr.io");
        }
        pull_github_image(state, credentials, image, logger).await?;
    } else {
        docker_pull(state, image, logger).await?;
    }

    let image_digest = docker_image_digest(state, image, logger).await?;
    let declared_volumes = docker_image_declared_volumes(state, image, logger).await?;
    if !declared_volumes.is_empty() {
        bail!(
            "Docker app images with declared volumes are not supported because their storage cannot be capped at 10 GiB"
        );
    }
    let exposed = docker_image_exposed_ports(state, image, logger).await?;
    let app_port = if honor_requested_port {
        app_port
    } else {
        resolve_container_port(app_port, &exposed)
    };
    log_deployment(
        logger,
        "Configure port",
        &format!("Using container port {app_port} for the service."),
    )
    .await?;
    if database.is_some() {
        log_deployment(
            logger,
            "Configure environment",
            "Attaching the private PostgreSQL environment variables.",
        )
        .await?;
    }

    let container_name = format!("knotree-app-{}", service_id.simple());
    if let Some(previous_container_name) = previous_container_name {
        if previous_container_name != container_name {
            remove_existing_container(state, previous_container_name, logger).await?;
        }
    }
    remove_existing_container(state, &container_name, logger).await?;
    let publish = format!("{}::{}", state.config.app_service_bind_address, app_port);
    let mut docker_args = vec![
        "run".to_owned(),
        "--detach".to_owned(),
        "--name".to_owned(),
        container_name.clone(),
        "--label".to_owned(),
        "com.knotree.managed-by=knotree-api".to_owned(),
        "--label".to_owned(),
        format!("com.knotree.project-id={project_id}"),
        "--label".to_owned(),
        "com.knotree.resource-type=app".to_owned(),
        "--restart".to_owned(),
        "unless-stopped".to_owned(),
        "--network".to_owned(),
        network_name,
        "--network-alias".to_owned(),
        cluster::PROJECT_NETWORK_APP_ALIAS.to_owned(),
        "--publish".to_owned(),
        publish,
    ];
    docker_args.extend(cluster::docker_resource_limit_args());
    log_deployment(
        logger,
        "Apply resource limits",
        "Capping the service at 1 vCPU, 1 GiB RAM, and 10 GiB writable storage.",
    )
    .await?;
    // Keep database credentials inside the container environment. They are
    // never serialized into the API response or Docker labels.
    for variable in database_environment {
        docker_args.push("--env".to_owned());
        docker_args.push(variable);
    }
    docker_args.push(image.to_owned());
    log_deployment(
        logger,
        "Start container",
        "Starting the application container.",
    )
    .await?;
    run_docker_for_deployment(state, logger, "Start container", docker_args).await?;

    let container_state = docker_inspect_running(state, &container_name, logger).await?;
    if !is_running_container_state(&container_state) {
        let _ = remove_existing_container(state, &container_name, logger).await;
        bail!("Docker app container is not running (status: {container_state})");
    }
    let port = match docker_port(state, &container_name, app_port, logger).await {
        Ok(port) => port,
        Err(error) => {
            let _ = remove_existing_container(state, &container_name, logger).await;
            return Err(error);
        }
    };

    Ok(ProvisionedAppService {
        host: state.config.app_service_public_host.clone(),
        port,
        app_port,
        container_name,
        image_digest,
    })
}

async fn docker_image_declared_volumes(
    state: &AppState,
    image: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<Vec<String>> {
    log_deployment(logger, "Inspect image", "Checking the image storage paths.").await?;
    let output = run_docker_for_deployment(
        state,
        logger,
        "Inspect image",
        vec![
            "inspect".to_owned(),
            "--format={{json .Config.Volumes}}".to_owned(),
            image.to_owned(),
        ],
    )
    .await?;
    Ok(parse_declared_volumes(&output))
}

async fn docker_image_digest(
    state: &AppState,
    image: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<Option<String>> {
    log_deployment(logger, "Inspect image", "Reading the pulled image digest.").await?;
    let output = run_docker_for_deployment(
        state,
        logger,
        "Inspect image",
        vec![
            "image".to_owned(),
            "inspect".to_owned(),
            "--format={{.Id}}".to_owned(),
            image.to_owned(),
        ],
    )
    .await?;
    Ok(parse_image_digest(&output))
}

async fn docker_container_image_digest(
    state: &AppState,
    container_name: &str,
) -> Result<Option<String>> {
    let output = docker_raw(
        state,
        [
            "inspect".to_owned(),
            "--format={{.Image}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(parse_image_digest(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_image_digest(output: &str) -> Option<String> {
    let digest = output.trim();
    (!digest.is_empty() && digest.starts_with("sha256:")).then(|| digest.to_owned())
}

fn parse_declared_volumes(output: &str) -> Vec<String> {
    let mut volumes = serde_json::from_str::<serde_json::Value>(output)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .map(|volumes| volumes.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    volumes.sort_unstable();
    volumes
}

async fn docker_image_exposed_ports(
    state: &AppState,
    image: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<Vec<u16>> {
    log_deployment(logger, "Inspect image", "Reading the image exposed ports.").await?;
    let output = run_docker_for_deployment(
        state,
        logger,
        "Inspect image",
        vec![
            "inspect".to_owned(),
            "--format={{range $port, $_ := .Config.ExposedPorts}}{{$port}} {{end}}".to_owned(),
            image.to_owned(),
        ],
    )
    .await?;
    Ok(parse_exposed_ports(&output))
}

async fn docker_pull(
    state: &AppState,
    image: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<String> {
    log_deployment(logger, "Pull image", &format!("Pulling image {image}.")).await?;
    run_docker_for_deployment(
        state,
        logger,
        "Pull image",
        vec!["pull".to_owned(), image.to_owned()],
    )
    .await
}

async fn pull_github_image(
    state: &AppState,
    credentials: &GithubDockerCredentials,
    image: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<String> {
    let _registry_lock = state.github_registry_lock.lock().await;
    docker_login(state, credentials, logger).await?;
    let pull_result = docker_pull(state, image, logger).await;
    let logout_result = docker_logout(state, logger).await;
    let output = pull_result?;
    logout_result?;
    Ok(output)
}

async fn docker_login(
    state: &AppState,
    credentials: &GithubDockerCredentials,
    logger: Option<&DeploymentLogger>,
) -> Result<()> {
    log_deployment(
        logger,
        "Authenticate registry",
        &format!("Authenticating to ghcr.io as @{}.", credentials.login),
    )
    .await?;
    if let Some(logger) = logger {
        run_docker_streaming_with_input(
            state,
            logger,
            "Authenticate registry",
            vec![
                "login".to_owned(),
                "ghcr.io".to_owned(),
                "--username".to_owned(),
                credentials.login.clone(),
                "--password-stdin".to_owned(),
            ],
            credentials.access_token.as_bytes(),
        )
        .await?;
        return Ok(());
    }

    let output = docker_raw_with_input(
        state,
        [
            "login".to_owned(),
            "ghcr.io".to_owned(),
            "--username".to_owned(),
            credentials.login.clone(),
            "--password-stdin".to_owned(),
        ],
        credentials.access_token.as_bytes(),
    )
    .await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("unknown Docker registry error");
        bail!("Docker registry login failed: {detail}");
    }
    Ok(())
}

async fn docker_logout(state: &AppState, logger: Option<&DeploymentLogger>) -> Result<()> {
    log_deployment(logger, "Sign out registry", "Signing out of ghcr.io.").await?;
    run_docker_for_deployment(
        state,
        logger,
        "Sign out registry",
        vec!["logout".to_owned(), "ghcr.io".to_owned()],
    )
    .await
    .map(|_| ())
}

async fn remove_existing_container(
    state: &AppState,
    container_name: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<()> {
    log_deployment(
        logger,
        "Replace container",
        "Checking for an existing container.",
    )
    .await?;
    let output = docker_raw(state, ["inspect".to_owned(), container_name.to_owned()]).await?;
    if !output.status.success() {
        return Ok(());
    }
    run_docker_for_deployment(
        state,
        logger,
        "Replace container",
        vec![
            "rm".to_owned(),
            "--force".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await
    .map(|_| ())
}

async fn docker_inspect_running(
    state: &AppState,
    container_name: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<String> {
    log_deployment(
        logger,
        "Verify container",
        "Checking that the container is running.",
    )
    .await?;
    let output = run_docker_for_deployment(
        state,
        logger,
        "Verify container",
        vec![
            "inspect".to_owned(),
            "--format={{.State.Status}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    Ok(output.trim().to_owned())
}

fn is_running_container_state(state: &str) -> bool {
    state.trim().eq_ignore_ascii_case("running")
}

async fn docker_port(
    state: &AppState,
    container_name: &str,
    app_port: u16,
    logger: Option<&DeploymentLogger>,
) -> Result<u16> {
    log_deployment(logger, "Publish port", "Resolving the public service port.").await?;
    let output = run_docker_for_deployment(
        state,
        logger,
        "Publish port",
        vec![
            "port".to_owned(),
            container_name.to_owned(),
            format!("{app_port}/tcp"),
        ],
    )
    .await?;
    parse_published_port(&output).context("Docker did not publish an app service port")
}

fn parse_published_port(output: &str) -> Option<u16> {
    output
        .lines()
        .filter_map(|line| line.rsplit(':').next())
        .find_map(|value| value.trim().parse::<u16>().ok())
}

async fn run_docker_for_deployment(
    state: &AppState,
    logger: Option<&DeploymentLogger>,
    step: &str,
    args: Vec<String>,
) -> Result<String> {
    match logger {
        Some(logger) => run_docker_streaming(state, logger, step, args).await,
        None => run_docker(state, args).await,
    }
}

async fn read_docker_stream<R>(reader: R, logger: &DeploymentLogger, step: &str) -> Result<String>
where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    let mut output = String::new();
    while let Some(line) = lines.next_line().await? {
        if let Err(error) = logger.append(step, &line).await {
            tracing::warn!(error = %error, "could not persist app service deployment log line");
        }
        output.push_str(&line);
        output.push('\n');
    }
    Ok(output)
}

async fn run_docker_streaming(
    state: &AppState,
    logger: &DeploymentLogger,
    step: &str,
    args: Vec<String>,
) -> Result<String> {
    let mut child = Command::new(&state.config.database_cluster_docker_binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "could not execute app service Docker binary `{}`",
                state.config.database_cluster_docker_binary
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .context("Docker stdout was not captured")?;
    let stderr = child
        .stderr
        .take()
        .context("Docker stderr was not captured")?;
    let stdout_lines = read_docker_stream(stdout, logger, step);
    let stderr_lines = read_docker_stream(stderr, logger, step);
    let (stdout_result, stderr_result, status_result) =
        tokio::join!(stdout_lines, stderr_lines, child.wait());
    let stdout = stdout_result?;
    let stderr = stderr_result?;
    let status = status_result?;
    if !status.success() {
        let detail = stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("unknown Docker error");
        bail!("Docker app service operation failed: {detail}");
    }
    Ok(stdout.trim().to_owned())
}

async fn run_docker_streaming_with_input(
    state: &AppState,
    logger: &DeploymentLogger,
    step: &str,
    args: Vec<String>,
    input: &[u8],
) -> Result<()> {
    let mut child = Command::new(&state.config.database_cluster_docker_binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "could not execute app service Docker binary `{}`",
                state.config.database_cluster_docker_binary
            )
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).await?;
    }
    let stdout = child
        .stdout
        .take()
        .context("Docker stdout was not captured")?;
    let stderr = child
        .stderr
        .take()
        .context("Docker stderr was not captured")?;
    let stdout_lines = read_docker_stream(stdout, logger, step);
    let stderr_lines = read_docker_stream(stderr, logger, step);
    let (_, stderr_result, status_result) = tokio::join!(stdout_lines, stderr_lines, child.wait());
    let stderr = stderr_result?;
    let status = status_result?;
    if !status.success() {
        let detail = stderr
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("unknown Docker registry error");
        bail!("Docker registry login failed: {detail}");
    }
    Ok(())
}

async fn run_docker<I>(state: &AppState, args: I) -> Result<String>
where
    I: IntoIterator<Item = String>,
{
    let output = docker_raw(state, args).await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.lines().next().unwrap_or("unknown Docker error");
        bail!("Docker app service operation failed: {detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn docker_container_running(
    state: &AppState,
    container_name: &str,
) -> std::result::Result<bool, AppError> {
    let output = docker_raw(
        state,
        [
            "inspect".to_owned(),
            "--format={{.State.Status}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await
    .map_err(|error| {
        tracing::warn!(
            container_name,
            error = %error,
            "could not inspect app service container"
        );
        AppError::ServiceUnavailable {
            code: "APP_SERVICE_LOGS_UNAVAILABLE",
            message: "The app service logs are temporarily unavailable.",
        }
    })?;

    Ok(output.status.success()
        && is_running_container_state(&String::from_utf8_lossy(&output.stdout)))
}

fn docker_log_lines(output: &Output) -> Vec<String> {
    let mut contents = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.trim().is_empty() {
        if !contents.is_empty() && !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(&stderr);
    }
    contents.lines().map(str::to_owned).collect()
}

async fn docker_raw<I>(state: &AppState, args: I) -> Result<Output>
where
    I: IntoIterator<Item = String>,
{
    Command::new(&state.config.database_cluster_docker_binary)
        .args(args)
        .output()
        .await
        .with_context(|| {
            format!(
                "could not execute app service Docker binary `{}`",
                state.config.database_cluster_docker_binary
            )
        })
}

async fn docker_raw_with_input<const N: usize>(
    state: &AppState,
    args: [String; N],
    input: &[u8],
) -> Result<Output> {
    let mut child = Command::new(&state.config.database_cluster_docker_binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "could not execute app service Docker binary `{}`",
                state.config.database_cluster_docker_binary
            )
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input).await?;
    }
    child
        .wait_with_output()
        .await
        .context("Docker login failed")
}

fn advisory_lock_key(project_id: Uuid) -> i64 {
    let bytes = project_id.into_bytes();
    i64::from_be_bytes(bytes[..8].try_into().expect("uuid has eight leading bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_public_and_github_images() {
        assert_eq!(validate_image(" nginx:alpine ").unwrap(), "nginx:alpine");
        assert!(validate_image("https://docker.io/nginx").is_err());
        assert!(validate_image("nginx; echo leaked").is_err());
        assert!(validate_image_source("public", "nginx:alpine").is_ok());
        assert!(validate_image_source("github", "ghcr.io/acme/app:latest").is_ok());
        assert!(validate_image_source("github", "nginx:latest").is_err());
    }

    #[test]
    fn validates_container_port() {
        assert_eq!(validate_app_port(None).unwrap(), DEFAULT_APP_PORT);
        assert_eq!(validate_app_port(Some(8080)).unwrap(), 8080);
        assert!(validate_app_port(Some(0)).is_err());
        assert!(validate_app_port(Some(65_536)).is_err());
    }

    #[test]
    fn only_running_docker_states_are_ready_for_public_traffic() {
        assert!(is_running_container_state("running"));
        assert!(is_running_container_state("RUNNING\n"));
        assert!(!is_running_container_state("restarting"));
        assert!(!is_running_container_state("true"));
    }

    #[test]
    fn maps_publish_port_to_image_expose_when_requested_port_is_absent() {
        assert_eq!(parse_exposed_ports(r#"{"80/tcp":{}}"#), vec![80]);
        assert_eq!(parse_exposed_ports("80/tcp 443/tcp"), vec![80, 443]);
        assert_eq!(parse_exposed_ports(""), Vec::<u16>::new());
        assert_eq!(resolve_container_port(8080, &[80]), 80);
        assert_eq!(resolve_container_port(80, &[80, 443]), 80);
        assert_eq!(resolve_container_port(3000, &[]), 3000);
        assert_eq!(resolve_container_port(9000, &[8080, 8443]), 8080);
        assert_eq!(resolve_container_port(3000, &[80]), 80);
    }

    #[test]
    fn rejects_image_volume_paths_that_would_bypass_the_storage_cap() {
        assert_eq!(parse_declared_volumes("null"), Vec::<String>::new());
        assert_eq!(
            parse_declared_volumes(r#"{"/var/lib/app":{},"/cache":{}}"#),
            vec!["/cache".to_owned(), "/var/lib/app".to_owned()]
        );
    }

    #[test]
    fn uses_a_stable_container_name_per_service() {
        let service_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(
            format!("knotree-app-{}", service_id.simple()),
            "knotree-app-11111111222233334444555555555555"
        );
    }

    #[test]
    fn caps_a_project_at_six_app_services() {
        assert_eq!(MAX_APP_SERVICES_PER_PROJECT, 6);
    }

    #[test]
    fn reads_docker_image_identities() {
        assert_eq!(
            parse_image_digest(" sha256:0123456789abcdef\n"),
            Some("sha256:0123456789abcdef".to_owned())
        );
        assert_eq!(parse_image_digest(""), None);
        assert_eq!(parse_image_digest("latest"), None);
        assert_eq!(
            short_image_digest("sha256:0123456789abcdef"),
            "sha256:01234567…cdef"
        );
    }

    #[test]
    fn routes_public_proxy_requests_to_the_container_path() {
        let uri = "/api/v1/public/app-services/11111111-2222-3333-4444-555555555555/health?probe=1"
            .parse::<axum::http::Uri>()
            .unwrap();
        assert_eq!(public_upstream_path(&uri), "/health?probe=1");

        let nested_uri = "/public/app-services/11111111-2222-3333-4444-555555555555/health?probe=1"
            .parse::<axum::http::Uri>()
            .unwrap();
        assert_eq!(public_upstream_path(&nested_uri), "/health?probe=1");

        let root = "/api/v1/public/app-services/11111111-2222-3333-4444-555555555555"
            .parse::<axum::http::Uri>()
            .unwrap();
        assert_eq!(public_upstream_path(&root), "/");
        assert_eq!(url_host("127.0.0.1"), "127.0.0.1");
        assert_eq!(url_host("::1"), "[::1]");
    }

    #[test]
    fn resolves_one_random_subdomain_from_the_wildcard_host() {
        assert_eq!(
            public_subdomain_from_host("APP-0123456789ABCDEF.knotree.org:443", "knotree.org"),
            Some("app-0123456789abcdef".to_owned())
        );
        assert!(host_matches_public_domain(
            "app-0123456789abcdef.knotree.org",
            "knotree.org"
        ));
        assert!(!host_matches_public_domain(
            "app-0123456789abcdef.knotree.org.evil",
            "knotree.org"
        ));
        assert_eq!(
            public_subdomain_from_host("team.app-0123456789abcdef.knotree.org", "knotree.org"),
            None
        );
        assert_eq!(
            public_subdomain_from_host("knotree.org", "knotree.org"),
            None
        );
    }

    #[test]
    fn generates_dns_safe_random_app_subdomains() {
        let subdomain = new_public_subdomain();
        assert!(subdomain.starts_with("app-"));
        assert!(is_valid_public_subdomain(&subdomain));
        assert_eq!(subdomain.len(), 20);
    }

    #[test]
    fn returns_the_assigned_domain_for_a_ready_service() {
        let service = AppServiceRow {
            id: Uuid::new_v4(),
            public_subdomain: "app-0123456789abcdef".to_owned(),
            project_id: Uuid::new_v4(),
            name: "Web app".to_owned(),
            image: "nginx:alpine".to_owned(),
            image_source: IMAGE_SOURCE_PUBLIC.to_owned(),
            app_port: 80,
            host: Some("localhost".to_owned()),
            port: Some(53107),
            container_name: Some("knotree-app-test".to_owned()),
            status: STATUS_READY.to_owned(),
            error_message: None,
            database_resource_id: None,
            auto_deploy_enabled: false,
            deployed_image_digest: None,
            auto_deploy_checked_at: None,
            auto_deploy_error: None,
        };

        let response = app_service_response(
            &service,
            "localhost",
            8080,
            Some("knotree.org"),
            "https",
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            response.public_domain.as_deref(),
            Some("app-0123456789abcdef.knotree.org")
        );
        assert_eq!(
            response.service_url.as_deref(),
            Some("https://app-0123456789abcdef.knotree.org")
        );
    }

    #[test]
    fn preserves_the_full_path_for_wildcard_domain_requests() {
        let uri = "/api/v1/health?probe=1".parse::<axum::http::Uri>().unwrap();
        assert_eq!(public_domain_upstream_path(&uri), "/api/v1/health?probe=1");
        let root = "/".parse::<axum::http::Uri>().unwrap();
        assert_eq!(public_domain_upstream_path(&root), "/");
    }

    #[tokio::test]
    async fn renders_an_html_error_page_for_an_unknown_public_domain() {
        let response = public_domain_not_found_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/html; charset=utf-8")
        );
        let body = to_bytes(response.into_body(), 16 * 1024).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("App not found"));
        assert!(body.contains("There is no public app service assigned to this domain"));
    }

    #[test]
    fn parses_the_current_docker_published_port() {
        assert_eq!(parse_published_port("127.0.0.1:53107\n"), Some(53107));
        assert_eq!(
            parse_published_port("0.0.0.0:53107\n[::]:53107\n"),
            Some(53107)
        );
        assert_eq!(parse_published_port(""), None);
    }

    #[test]
    fn injects_database_variables_using_the_private_postgres_alias() {
        let key = [7_u8; 32];
        let database = DatabaseResourceRow {
            id: Uuid::new_v4(),
            name: "Postgres".to_owned(),
            database_name: "knotree_db_project".to_owned(),
            role_name: "knotree_role_project".to_owned(),
            password_ciphertext: security::encrypt_secret("secret", &key).unwrap(),
        };

        let variables = database_environment(&key, Some(&database)).unwrap();
        assert!(variables.contains(
            &"DATABASE_URL=postgres://knotree_role_project:secret@postgres:5432/knotree_db_project"
                .to_owned()
        ));
        assert!(variables.contains(&"PGHOST=postgres".to_owned()));
        assert!(variables.contains(&"PGPORT=5432".to_owned()));
        assert!(variables.contains(&"PGPASSWORD=secret".to_owned()));
    }

    #[test]
    fn exposes_saved_deployment_log_lines_in_order() {
        let deployment = AppServiceDeploymentRow {
            id: Uuid::new_v4(),
            status: STATUS_PROVISIONING.to_owned(),
            current_step: "Pull image".to_owned(),
            logs: "Deployment queued.\nPulling image nginx:alpine.\nlayer complete".to_owned(),
            error_message: None,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        };

        let response = deployment_response(&deployment);
        assert_eq!(response.current_step, "Pull image");
        assert_eq!(response.logs.len(), 3);
        assert_eq!(response.logs[1], "Pulling image nginx:alpine.");
    }
}
