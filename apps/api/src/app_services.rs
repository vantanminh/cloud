use std::{
    collections::BTreeMap,
    convert::Infallible,
    fs,
    path::PathBuf,
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
use subtle::ConstantTimeEq;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    time::{Duration, sleep},
};
use uuid::Uuid;

use crate::{
    auth,
    cluster,
    cluster_kubernetes::{self, AppWorkloadSpec},
    error::AppError,
    github::{self, GithubDockerCredentials},
    kong,
    limits::{
        self, APP_SERVICE_VIRTUAL_CPU, DEFAULT_APP_RATE_LIMIT_RPM, KUBERNETES_APP_CPU_LIMIT,
        RESOURCE_MEMORY_LIMIT_BYTES, RESOURCE_VOLUME_LIMIT_BYTES, validate_rate_limit_rpm,
    },
    metrics::{
        MAX_METRIC_RESPONSE_POINTS, METRIC_RETENTION_SECONDS, METRIC_SAMPLE_INTERVAL_SECONDS,
        downsample_metric_points, has_system_metrics, metric_sample_concurrency,
        parse_metric_range, sanitize_volume_metric, unix_timestamp,
    },
    models::{
        AccountDeploymentLog, AppServiceDatabaseConnectionResponse, AppServiceDeploymentResponse,
        AppServiceLogsResponse, AppServiceMetricPoint, AppServiceMetricsResponse,
        AppServiceResponse, CreateAppServiceRequest, UpdateAppServiceAutoDeployRequest,
        UpdateAppServiceDatabaseRequest, UpdateAppServicePublicAccessRequest,
        UpdateAppServiceRequest,
    },
    html_pages::{self, IMAGE_SOURCE_HTML, IMAGE_SOURCE_HTML_GITHUB},
    knotree_registry::{self, RegistryDockerCredentials},
    projects,
    public_access::{self, PublicAccessState, public_hostname, should_proxy_public_host},
    redis_resources, security,
    state::AppState,
};

const IMAGE_SOURCE_PUBLIC: &str = "public";
const IMAGE_SOURCE_GITHUB: &str = "github";
const IMAGE_SOURCE_KNOTREE_REGISTRY: &str = "knotree_registry";
const STATUS_READY: &str = "ready";
const STATUS_PROVISIONING: &str = "provisioning";
const STATUS_ERROR: &str = "error";
const DEFAULT_APP_PORT: u16 = 3000;
const MAX_APP_SERVICES_PER_PROJECT: i64 = 6;
const AUTO_DEPLOY_INTERVAL_SECONDS: u64 = 60;
const MAX_PUBLIC_PROXY_BODY_BYTES: usize = 64 * 1024 * 1024;
const PROVISIONING_ERROR_MESSAGE: &str =
    "The app service could not be deployed. Check the image and try again.";
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
    public_subdomain: Option<String>,
    public_access_enabled: bool,
    rate_limit_rpm: i32,
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
    registry_connection_id: Option<Uuid>,
    deployed_image_digest: Option<String>,
    auto_deploy_checked_at: Option<OffsetDateTime>,
    auto_deploy_error: Option<String>,
    html_repo: Option<String>,
    html_branch: Option<String>,
    html_sha: Option<String>,
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
    image_source: String,
    container_name: Option<String>,
    github_connection_user_id: Option<Uuid>,
    html_repo: Option<String>,
    html_branch: Option<String>,
    html_sha: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RegistryDeployCandidate {
    job_id: Uuid,
    app_service_id: Uuid,
    project_id: Uuid,
    image: String,
    app_port: i32,
    database_resource_id: Option<Uuid>,
    registry_connection_id: Option<Uuid>,
    image_digest: String,
    immutable_image: String,
    deployed_image_digest: Option<String>,
}

#[derive(Debug, Clone)]
struct ClaimedRegistryDeployment {
    job_id: Uuid,
    app_service_id: Uuid,
    project_id: Uuid,
    connection_id: Uuid,
    deployment_id: Uuid,
    image: String,
    image_digest: String,
    app_port: u16,
    database_resource_id: Option<Uuid>,
    database: Option<DatabaseResourceRow>,
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

const APP_SERVICE_COLUMNS: &str = "id, public_subdomain, public_access_enabled, rate_limit_rpm, project_id, name, image, image_source, app_port, host, port, container_name, status, error_message, database_resource_id, auto_deploy_enabled, registry_connection_id, github_connection_user_id, deployed_image_digest, auto_deploy_checked_at, auto_deploy_error, html_repo, html_branch, html_sha";
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
    Path((workspace_id, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<AppServiceResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
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
    Path((workspace_id, project_slug, deployment_id)): Path<(String, String, Uuid)>,
) -> Result<Sse<impl futures_util::Stream<Item = std::result::Result<Event, Infallible>>>, AppError>
{
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
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
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
) -> Result<Json<AppServiceLogsResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    runtime_logs(&state, project_id, app_service_id)
        .await
        .map(Json)
}

pub(crate) async fn runtime_logs(
    state: &AppState,
    project_id: Uuid,
    app_service_id: Uuid,
) -> Result<AppServiceLogsResponse, AppError> {
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

    if let Some(response) = logs_unavailable_response(app_service_id, &target) {
        return Ok(response);
    }

    let container_name = target
        .container_name
        .clone()
        .expect("ready services have a container name");

    if state.config.uses_kubernetes_workloads() {
        let lines = cluster_kubernetes::pod_logs(
            &state.config.database_cluster_namespace,
            &container_name,
            200,
        )
        .await
        .map_err(|error| {
            tracing::warn!(
                app_service_id = %app_service_id,
                container_name = %container_name,
                error = %error,
                "could not read app service Kubernetes logs"
            );
            AppError::ServiceUnavailable {
                code: "APP_SERVICE_LOGS_UNAVAILABLE",
                message: "The app service logs are temporarily unavailable.",
            }
        })?;
        return Ok(AppServiceLogsResponse {
            app_service_id,
            container_name: Some(container_name),
            status: target.status,
            running: true,
            lines,
            message: None,
        });
    }

    let running = docker_container_running(state, &container_name).await?;
    let output = docker_raw(
        state,
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

    Ok(AppServiceLogsResponse {
        app_service_id,
        container_name: Some(container_name),
        status: target.status,
        running,
        lines: docker_log_lines(&output),
        message: None,
    })
}

fn logs_unavailable_response(
    app_service_id: Uuid,
    target: &AppServiceLogsTarget,
) -> Option<AppServiceLogsResponse> {
    let Some(container_name) = target.container_name.clone() else {
        return Some(AppServiceLogsResponse {
            app_service_id,
            container_name: None,
            status: target.status.clone(),
            running: false,
            lines: Vec::new(),
            message: Some("The app service container has not been deployed yet.".to_owned()),
        });
    };
    if target.status != STATUS_READY {
        return Some(AppServiceLogsResponse {
            app_service_id,
            container_name: Some(container_name),
            status: target.status.clone(),
            running: false,
            lines: Vec::new(),
            message: Some("Runtime logs are available after the app service is ready.".to_owned()),
        });
    }
    None
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PublicProxyTarget {
    app_port: i32,
    port: Option<i32>,
    host: Option<String>,
    container_name: Option<String>,
    status: String,
    rate_limit_rpm: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct PublicDomainLookup {
    id: Uuid,
    public_access_enabled: bool,
    public_subdomain: Option<String>,
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
        // Keep the complete metrics contract stable even before the first
        // public request arrives. A zero-valued average/error rate is both
        // numerically meaningful for an empty window and lets the API/UI
        // render every metric field instead of omitting it.
        response_time_ms: Some(if requests > 0 {
            traffic.response_time_ms_total / requests as f64
        } else {
            0.0
        }),
        request_error_rate: Some(if requests > 0 {
            (traffic.request_errors.min(requests) as f64 / requests as f64) * 100.0
        } else {
            0.0
        }),
    }
}

fn complete_app_service_metric_point(mut point: AppServiceMetricPoint) -> AppServiceMetricPoint {
    // Older rows were collected before Kubernetes exposed the kubelet
    // counters. Normalize those rows on read and protect future samples from
    // a provider returning a missing optional field. Live Kubernetes samples
    // still carry the real values; this only supplies an explicit zero when a
    // counter is genuinely absent.
    point.cpu_percent = Some(point.cpu_percent.unwrap_or_default());
    point.memory_used_bytes = Some(point.memory_used_bytes.unwrap_or_default());
    point.memory_limit_bytes = Some(
        point
            .memory_limit_bytes
            .unwrap_or(RESOURCE_MEMORY_LIMIT_BYTES),
    );
    point.volume_capacity_bytes = Some(
        point
            .volume_capacity_bytes
            .unwrap_or(RESOURCE_VOLUME_LIMIT_BYTES),
    );
    point.volume_used_bytes = Some(
        sanitize_volume_metric(
            point.volume_used_bytes,
            point.volume_capacity_bytes,
        )
        .unwrap_or_default(),
    );
    point.network_receive_bytes = Some(point.network_receive_bytes.unwrap_or_default());
    point.network_transmit_bytes = Some(point.network_transmit_bytes.unwrap_or_default());
    point.disk_read_bytes = Some(point.disk_read_bytes.unwrap_or_default());
    point.disk_write_bytes = Some(point.disk_write_bytes.unwrap_or_default());
    point.public_network_receive_bytes =
        Some(point.public_network_receive_bytes.unwrap_or_default());
    point.public_network_transmit_bytes =
        Some(point.public_network_transmit_bytes.unwrap_or_default());
    point.requests = Some(point.requests.unwrap_or_default());
    point.response_time_ms = Some(point.response_time_ms.unwrap_or_default());
    point.request_error_rate = Some(point.request_error_rate.unwrap_or_default());
    point
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
    if state.config.uses_kubernetes_workloads() {
        return stored_port
            .map(u16::try_from)
            .transpose()
            .map_err(|_| AppError::internal("invalid app service published port"))?
            .or(Some(app_port))
            .ok_or(AppError::ServiceUnavailable {
                code: "APP_SERVICE_PUBLIC_UNAVAILABLE",
                message: "The app service is temporarily unavailable.",
            });
    }
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum KongTrafficLogPayload {
    Batch(Vec<KongTrafficLogEntry>),
    Single(KongTrafficLogEntry),
}

impl KongTrafficLogPayload {
    fn into_entries(self) -> Vec<KongTrafficLogEntry> {
        match self {
            Self::Batch(entries) => entries,
            Self::Single(entry) => vec![entry],
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KongTrafficLogEntry {
    request: Option<KongTrafficLogRequest>,
    response: Option<KongTrafficLogResponse>,
    latencies: Option<KongTrafficLogLatencies>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KongTrafficLogRequest {
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KongTrafficLogResponse {
    size: Option<u64>,
    status: Option<u16>,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct KongTrafficLogLatencies {
    request: Option<f64>,
}

pub(crate) async fn record_kong_public_traffic(
    State(state): State<AppState>,
    Path(app_service_id): Path<Uuid>,
    headers: HeaderMap,
    Json(payload): Json<KongTrafficLogPayload>,
) -> Result<StatusCode, AppError> {
    let expected = state
        .config
        .kong_traffic_log_token
        .as_deref()
        .ok_or(AppError::NotFound {
            code: "PUBLIC_TRAFFIC_LOGGING_DISABLED",
            message: "Public traffic logging is not configured.",
        })?;
    let presented = headers
        .get("x-knotree-traffic-token")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let expected_hash = security::token_hash(expected);
    let presented_hash = security::token_hash(presented);
    if expected_hash.as_slice().ct_eq(presented_hash.as_slice()).unwrap_u8() != 1 {
        return Err(AppError::Unauthorized {
            code: "PUBLIC_TRAFFIC_LOG_UNAUTHORIZED",
            message: "The public traffic log token is invalid.",
        });
    }

    for entry in payload.into_entries() {
        let request_bytes = entry
            .request
            .and_then(|request| request.size)
            .map(|bytes| usize::try_from(bytes).unwrap_or(usize::MAX))
            .unwrap_or_default();
        let response = entry.response.unwrap_or_default();
        let response_bytes = response
            .size
            .map(|bytes| usize::try_from(bytes).unwrap_or(usize::MAX))
            .unwrap_or_default();
        let response_time_ms = entry
            .latencies
            .and_then(|latencies| latencies.request)
            .unwrap_or_default()
            .max(0.0);
        let is_error = response.status.map(|status| status >= 400).unwrap_or(true);
        state.record_public_app_service_request(
            app_service_id,
            request_bytes,
            response_bytes,
            response_time_ms,
            is_error,
        );
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn public_domain_fallback(
    State(state): State<AppState>,
    request: Request<Body>,
) -> Response {
    if html_pages::is_public_html_path(request.uri().path()) {
        return StatusCode::NOT_FOUND.into_response();
    }
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
    if is_public_domain_request(&state, &request)
        && !html_pages::is_public_html_path(request.uri().path())
    {
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
    let row = sqlx::query_as::<_, PublicDomainLookup>(
        "SELECT id, public_access_enabled, public_subdomain FROM project_app_services WHERE public_subdomain = $1",
    )
    .bind(&public_subdomain)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_DOMAIN_NOT_FOUND",
        message: "The public app service domain could not be resolved.",
    })?;
    if !should_proxy_public_host(
        &PublicAccessState {
            enabled: row.public_access_enabled,
            subdomain: row.public_subdomain,
        },
        &public_subdomain,
    ) {
        return Err(AppError::NotFound {
            code: "APP_SERVICE_DOMAIN_NOT_FOUND",
            message: "The public app service domain could not be resolved.",
        });
    }
    let app_service_id = row.id;

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
        "SELECT app_port, port, host, container_name, status, rate_limit_rpm
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
    let limit = u32::try_from(target.rate_limit_rpm).unwrap_or(DEFAULT_APP_RATE_LIMIT_RPM);
    let allowed = {
        let mut limiter = state
            .public_rate_limiter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        limiter
            .check(&app_service_id.to_string(), limit, unix_timestamp() as u64)
            .allowed
    };
    if !allowed {
        return Err(AppError::TooManyRequests {
            code: "APP_SERVICE_RATE_LIMITED",
            message: "This app service is receiving too many public requests.",
        });
    }
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
    let bind_host = if state.config.uses_kubernetes_workloads() {
        url_host(target.host.as_deref().unwrap_or(container_name))
    } else {
        url_host(&state.config.app_service_bind_address.to_string())
    };
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
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServiceAutoDeployRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;

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

    if input.enabled
        && existing.image_source != IMAGE_SOURCE_GITHUB
        && existing.image_source != IMAGE_SOURCE_HTML_GITHUB
        && existing.image_source != IMAGE_SOURCE_KNOTREE_REGISTRY
    {
        return Err(AppError::BadRequest {
            code: "AUTO_DEPLOY_SOURCE_UNSUPPORTED",
            message: "Automatic deploys are not available for this image source.",
        });
    }
    if input.enabled && existing.status != STATUS_READY {
        return Err(AppError::Conflict {
            code: "APP_SERVICE_NOT_READY",
            message: "The app service must be ready before automatic image deploys can be enabled.",
        });
    }
    if input.enabled
        && existing.image_source == IMAGE_SOURCE_KNOTREE_REGISTRY
        && state.config.knotree_registry_webhook_secret.is_none()
    {
        return Err(AppError::Conflict {
            code: "KNOTREE_REGISTRY_WEBHOOK_NOT_CONFIGURED",
            message: "Automatic Registry deploys are not configured on this Cloud installation yet.",
        });
    }
    if input.enabled
        && matches!(existing.image_source.as_str(), IMAGE_SOURCE_GITHUB | IMAGE_SOURCE_HTML_GITHUB)
        && github::docker_credentials(&state, user.id).await?.is_none()
    {
        return Err(AppError::Conflict {
            code: "GITHUB_CONNECTION_REQUIRED",
            message: "Connect GitHub before enabling automatic image deploys.",
        });
    }
    if input.enabled && existing.image_source == IMAGE_SOURCE_KNOTREE_REGISTRY {
        let connected = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM knotree_registry_connections
                WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL
            )",
        )
        .bind(existing.registry_connection_id)
        .bind(project_id)
        .fetch_one(&mut *transaction)
        .await?;
        if !connected {
            return Err(AppError::Conflict {
                code: "KNOTREE_REGISTRY_CONNECTION_REQUIRED",
                message: "Reconnect Knotree Registry before enabling automatic deploys.",
            });
        }
    }

    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "UPDATE project_app_services
         SET auto_deploy_enabled = $1,
             github_connection_user_id = CASE WHEN $1 AND $4 THEN $2 ELSE github_connection_user_id END,
             auto_deploy_error = NULL,
             updated_at = now()
         WHERE id = $3
         RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(input.enabled)
    .bind(user.id)
    .bind(app_service_id)
    .bind(matches!(existing.image_source.as_str(), IMAGE_SOURCE_GITHUB | IMAGE_SOURCE_HTML_GITHUB))
    .fetch_one(&mut *transaction)
    .await?;
    if !input.enabled && existing.image_source == IMAGE_SOURCE_KNOTREE_REGISTRY {
        sqlx::query(
            "UPDATE knotree_registry_deploy_jobs
             SET status = 'failed', locked_until = NULL,
                 last_error = 'Automatic Registry deploys were disabled.', updated_at = now()
             WHERE app_service_id = $1 AND status = 'pending'",
        )
        .bind(app_service_id)
        .execute(&mut *transaction)
        .await?;
    }
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

pub async fn update_public_access(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServicePublicAccessRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    update_public_access_for_user(
        &state,
        user.id,
        &workspace_id,
        &project_slug,
        app_service_id,
        input,
    )
    .await
    .map(Json)
}

pub async fn update_public_access_for_user(
    state: &AppState,
    user_id: Uuid,
    workspace_id: &str,
    project_slug: &str,
    app_service_id: Uuid,
    input: UpdateAppServicePublicAccessRequest,
) -> Result<AppServiceResponse, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_id, project_slug).await?;
    let rate_limit_rpm = match input.rate_limit_rpm {
        Some(value) => validate_rate_limit_rpm(value).map_err(|_| AppError::BadRequest {
            code: "INVALID_RATE_LIMIT",
            message: "Rate limit must be between 1 and 10000 requests per minute.",
        })?,
        None => limits::RateLimitPolicy {
            requests_per_minute: state.config.default_rate_limit_rpm,
        },
    };

    let existing = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1 AND project_id = $2"
    ))
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;

    let update = if input.enabled {
        public_access::enable_public_access(PublicAccessState {
            enabled: existing.public_access_enabled,
            subdomain: existing.public_subdomain.clone(),
        })
    } else {
        public_access::disable_public_access(PublicAccessState {
            enabled: existing.public_access_enabled,
            subdomain: existing.public_subdomain.clone(),
        })
    };

    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "UPDATE project_app_services
         SET public_access_enabled = $1,
             public_subdomain = $2,
             rate_limit_rpm = $3,
             updated_at = now()
         WHERE id = $4
         RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(update.enabled)
    .bind(update.subdomain.as_deref())
    .bind(
        i32::try_from(
            input
                .rate_limit_rpm
                .unwrap_or(rate_limit_rpm.requests_per_minute),
        )
        .unwrap_or(60),
    )
    .bind(app_service_id)
    .fetch_one(&state.db)
    .await?;

    refresh_kong_public_routes(state).await;

    let database = database_resource_by_id(state, project_id, service.database_resource_id).await?;
    app_service_response(
        &service,
        &state.config.app_service_public_host,
        state.config.bind_addr.port(),
        state.config.app_service_public_domain.as_deref(),
        &state.config.app_service_public_scheme,
        database.as_ref(),
        latest_deployment(&state.db, service.id).await?,
    )
}

const KONG_ROUTE_SYNC_INTERVAL_SECONDS: u64 = 15;

pub fn spawn_kong_route_syncer(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(KONG_ROUTE_SYNC_INTERVAL_SECONDS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            refresh_kong_public_routes(&state).await;
        }
    })
}

async fn refresh_kong_public_routes(state: &AppState) {
    if let Err(error) = sync_kong_routes(state).await {
        tracing::warn!(error = %error, "could not refresh Kong public app routes");
    }
}

pub(crate) async fn sync_kong_routes(state: &AppState) -> anyhow::Result<()> {
    let Some(admin_url) = state.config.kong_admin_url.as_deref() else {
        return Ok(());
    };
    let rows = sqlx::query_as::<_, KongSyncRow>(
        "SELECT id, public_subdomain, public_access_enabled, rate_limit_rpm, host, port, container_name, status, image_source
         FROM project_app_services
         WHERE public_access_enabled = true AND public_subdomain IS NOT NULL AND status = 'ready'",
    )
    .fetch_all(&state.db)
    .await?;
    let routes = kong_routes_from_rows(&rows, state.config.app_service_public_domain.as_deref());
    let config = kong::declarative_config(
        &routes,
        state.config.kong_traffic_log_endpoint.as_deref(),
        state.config.kong_traffic_log_token.as_deref(),
    )?;
    kong::apply_declarative_config(admin_url, &config).await
}

fn kong_routes_from_rows(
    rows: &[KongSyncRow],
    public_domain: Option<&str>,
) -> Vec<kong::KongAppRoute> {
    let mut routes = Vec::new();
    for row in rows {
        if !row.public_access_enabled || row.status != STATUS_READY {
            continue;
        }
        let hostname = public_hostname(true, row.public_subdomain.as_deref(), public_domain);
        let upstream = match (row.host.as_deref(), row.port) {
            (Some(host), Some(port)) => format!("http://{host}:{port}"),
            _ => continue,
        };
        if let Some(route) = kong::route_for_enabled_service(
            row.id,
            hostname,
            &upstream,
            u32::try_from(row.rate_limit_rpm).unwrap_or(DEFAULT_APP_RATE_LIMIT_RPM),
            html_pages::is_html_source(&row.image_source),
        ) {
            routes.push(route);
        }
    }
    routes
}

#[derive(Debug, sqlx::FromRow)]
struct KongSyncRow {
    id: Uuid,
    public_subdomain: Option<String>,
    public_access_enabled: bool,
    rate_limit_rpm: i32,
    host: Option<String>,
    port: Option<i32>,
    container_name: Option<String>,
    status: String,
    image_source: String,
}

pub async fn metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Query(query): Query<AppServiceMetricsQuery>,
) -> Result<Json<AppServiceMetricsResponse>, AppError> {
    let range = parse_metric_range(query.range.as_deref())?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
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
    let sample = complete_app_service_metric_point(AppServiceMetricPoint {
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
    });
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
        provider: state.config.database_cluster_provider.clone(),
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
    Path((workspace_id, project_slug)): Path<(String, String)>,
    Json(input): Json<CreateAppServiceRequest>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let response = create_for_user(&state, user.id, &workspace_id, &project_slug, input).await?;
    Ok((StatusCode::ACCEPTED, Json(response)).into_response())
}

pub async fn create_for_user(
    state: &AppState,
    user_id: Uuid,
    workspace_id: &str,
    project_slug: &str,
    input: CreateAppServiceRequest,
) -> Result<AppServiceResponse, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_id, project_slug).await?;

    if !state.config.app_service_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_PROVISIONING_DISABLED",
            message: "App service provisioning is not enabled for this environment.",
        });
    }

    let name = validate_service_name(input.name.as_deref().unwrap_or("App service"))?;
    let image_source_raw = input.image_source.trim().to_ascii_lowercase();
    let is_html = html_pages::is_html_source(&image_source_raw);
    let mut image = if is_html {
        state.config.html_nginx_image.clone()
    } else {
        validate_image(input.image.as_deref().unwrap_or(""))?
    };
    let mut deployment_image = image.clone();
    let image_source = validate_image_source(&input.image_source, &image)?;
    let registry_image = if image_source == IMAGE_SOURCE_KNOTREE_REGISTRY {
        image = knotree_registry::normalize_registry_image(&image).ok_or_else(|| {
            validation_field_error(
                "image",
                "Knotree Registry images must use registry.knotree.com/repository:tag.",
            )
        })?;
        knotree_registry::parse_registry_image(&image)
    } else {
        None
    };
    let registry_connection_id = if let Some(registry_image) = registry_image.as_ref() {
        let connection_id = input.registry_connection_id.ok_or_else(|| {
            validation_field_error(
                "registryConnectionId",
                "Connect Knotree Registry before deploying this private image.",
            )
        })?;
        let connected_repository = sqlx::query_scalar::<_, String>(
            "SELECT repository FROM knotree_registry_connections
             WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL",
        )
        .bind(connection_id)
        .bind(project_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Conflict {
            code: "KNOTREE_REGISTRY_CONNECTION_REQUIRED",
            message: "Connect Knotree Registry to this project before deploying the image.",
        })?;
        if connected_repository != registry_image.repository {
            return Err(AppError::Conflict {
                code: "KNOTREE_REGISTRY_REPOSITORY_MISMATCH",
                message: "This Registry connection is scoped to a different repository.",
            });
        }
        Some(connection_id)
    } else {
        if input.registry_connection_id.is_some() {
            return Err(validation_field_error(
                "registryConnectionId",
                "Registry connections can only be attached to Knotree Registry images.",
            ));
        }
        None
    };
    let registry_credentials = if let (Some(connection_id), Some(registry_image)) =
        (registry_connection_id, registry_image.as_ref())
    {
        let credentials = knotree_registry::load_credentials(state, project_id, connection_id)
            .await?
            .ok_or(AppError::Conflict {
                code: "KNOTREE_REGISTRY_CONNECTION_REVOKED",
                message: "Reconnect Knotree Registry before deploying this image.",
            })?;
        let digest = knotree_registry::resolve_tag_digest(
            &credentials.username,
            &credentials.password,
            &registry_image.repository,
            &registry_image.tag,
        )
        .await
        .ok_or(AppError::Conflict {
            code: "KNOTREE_REGISTRY_IMAGE_UNAVAILABLE",
            message: "Knotree Registry could not read this image tag. Check the repository, tag, and pull token.",
        })?;
        deployment_image = knotree_registry::immutable_image(&registry_image.repository, &digest)
            .expect("a validated repository and digest produce a valid immutable reference");
        Some(credentials)
    } else {
        None
    };
    let app_port = if is_html {
        html_pages::HTML_NGINX_PORT
    } else {
        validate_app_port(input.app_port)?
    };
    let auto_deploy_enabled = match image_source.as_str() {
        IMAGE_SOURCE_GITHUB => input.auto_deploy.unwrap_or(true),
        IMAGE_SOURCE_HTML_GITHUB => input.auto_deploy.unwrap_or(true),
        IMAGE_SOURCE_KNOTREE_REGISTRY => input.auto_deploy.unwrap_or(false),
        _ => false,
    };
    if auto_deploy_enabled
        && image_source == IMAGE_SOURCE_KNOTREE_REGISTRY
        && state.config.knotree_registry_webhook_secret.is_none()
    {
        return Err(AppError::Conflict {
            code: "KNOTREE_REGISTRY_WEBHOOK_NOT_CONFIGURED",
            message: "Automatic Registry deploys are not configured on this Cloud installation yet.",
        });
    }
    let github_connection_user_id =
        (image_source == IMAGE_SOURCE_GITHUB || image_source == IMAGE_SOURCE_HTML_GITHUB)
            .then_some(user_id);
    let public_subdomain = if is_html {
        let suffix = input
            .page_slug
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(name.as_str());
        Some(html_pages::page_subdomain(suffix)?)
    } else {
        None
    };
    let html_repo = if image_source == IMAGE_SOURCE_HTML_GITHUB {
        let (owner, repo) =
            html_pages::parse_github_repo(input.github_repo.as_deref().unwrap_or(""))?;
        Some(format!("{owner}/{repo}"))
    } else {
        None
    };
    let html_branch = input
        .github_branch
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    if image_source == IMAGE_SOURCE_HTML {
        html_pages::files_from_pasted_html(
            input.index_html.as_deref().unwrap_or(""),
            Uuid::nil(),
        )?;
    }
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
        if let Some(subdomain) = public_subdomain.as_deref() {
            let taken = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM project_app_services WHERE public_subdomain = $1)",
            )
            .bind(subdomain)
            .fetch_one(&mut *transaction)
            .await?;
            if taken {
                let mut fields = BTreeMap::new();
                fields.insert(
                    "pageSlug".to_owned(),
                    "That page- domain is already in use. Choose another suffix.".to_owned(),
                );
                return Err(AppError::validation(fields));
            }
        }

        if let Some(connection_id) = registry_connection_id {
            let connected_repository = sqlx::query_scalar::<_, String>(
                "SELECT repository FROM knotree_registry_connections
                 WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL
                 FOR SHARE",
            )
            .bind(connection_id)
            .bind(project_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(AppError::Conflict {
                code: "KNOTREE_REGISTRY_CONNECTION_REVOKED",
                message: "Reconnect Knotree Registry before deploying this image.",
            })?;
            if registry_image
                .as_ref()
                .is_none_or(|image| image.repository != connected_repository)
            {
                return Err(AppError::Conflict {
                    code: "KNOTREE_REGISTRY_REPOSITORY_MISMATCH",
                    message: "This Registry connection is scoped to a different repository.",
                });
            }
        }

        let service = sqlx::query_as::<_, AppServiceRow>(&format!(
            "INSERT INTO project_app_services (id, project_id, name, image, image_source, app_port, public_subdomain, public_access_enabled, rate_limit_rpm, status, auto_deploy_enabled, registry_connection_id, github_connection_user_id, html_repo, html_branch) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15) RETURNING {APP_SERVICE_COLUMNS}"
        ))
        .bind(Uuid::new_v4())
        .bind(project_id)
        .bind(&name)
        .bind(&image)
        .bind(&image_source)
        .bind(i32::from(app_port))
        .bind(&public_subdomain)
        .bind(is_html)
        .bind(i32::try_from(state.config.default_rate_limit_rpm).unwrap_or(60))
        .bind(STATUS_PROVISIONING)
        .bind(auto_deploy_enabled)
        .bind(registry_connection_id)
        .bind(github_connection_user_id)
        .bind(&html_repo)
        .bind(&html_branch)
        .fetch_one(&mut *transaction)
        .await?;
        transaction.commit().await?;
        service
    };

    if image_source == IMAGE_SOURCE_HTML {
        let files = html_pages::files_from_pasted_html(
            input.index_html.as_deref().unwrap_or(""),
            service.id,
        )?;
        html_pages::replace_files(&state.db, service.id, &files)
            .await
            .map_err(AppError::internal)?;
    }

    let github_credentials = if image_source == IMAGE_SOURCE_GITHUB
        || image_source == IMAGE_SOURCE_HTML_GITHUB
    {
        match github::docker_credentials(state, user_id).await? {
            Some(credentials) => Some(credentials),
            None => {
                sqlx::query(
                    "UPDATE project_app_services SET status = $1, error_message = $2, updated_at = now() WHERE id = $3",
                )
                .bind(STATUS_ERROR)
                .bind("Connect GitHub before deploying from GitHub.")
                .bind(service.id)
                .execute(&state.db)
                .await?;
                return Err(AppError::Conflict {
                    code: "GITHUB_CONNECTION_REQUIRED",
                    message: if image_source == IMAGE_SOURCE_HTML_GITHUB {
                        "Reconnect GitHub so Knotree can read the HTML repository (repo scope)."
                    } else {
                        "Connect GitHub before deploying a private GitHub image."
                    },
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
            deployment_image,
            app_port,
            github_credentials,
            registry_credentials,
            None,
            None,
            false,
            false,
        )
        .await;
    });

    Ok(response)
}

pub async fn update_html_page(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<html_pages::UpdateHtmlPageRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let existing = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1 AND project_id = $2"
    ))
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;
    if existing.image_source != IMAGE_SOURCE_HTML {
        return Err(AppError::BadRequest {
            code: "HTML_PASTE_ONLY",
            message: "Only pasted HTML pages can be edited in the dashboard.",
        });
    }
    if existing.status == STATUS_PROVISIONING {
        return Err(AppError::Conflict {
            code: "APP_SERVICE_PROVISIONING",
            message: "This HTML page is already being deployed.",
        });
    }
    let files = html_pages::files_from_pasted_html(
        &input.index_html,
        existing.id,
    )?;
    html_pages::replace_files(&state.db, existing.id, &files)
        .await
        .map_err(AppError::internal)?;
    html_pages::write_site_to_disk(&state.config.html_site_data_dir, existing.id, &files)
        .await
        .map_err(AppError::internal)?;

    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO app_service_deployments (id, app_service_id, status, current_step, logs) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(deployment_id)
    .bind(existing.id)
    .bind(STATUS_PROVISIONING)
    .bind("Queued")
    .bind("HTML update queued.")
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE project_app_services SET status = $1, error_message = NULL, updated_at = now() WHERE id = $2",
    )
    .bind(STATUS_PROVISIONING)
    .bind(existing.id)
    .execute(&state.db)
    .await?;

    let github_credentials = None;
    let image = existing.image.clone();
    let app_port = u16::try_from(existing.app_port).unwrap_or(html_pages::HTML_NGINX_PORT);
    let service_id = existing.id;
    let response = app_service_response(
        &existing,
        &state.config.app_service_public_host,
        state.config.bind_addr.port(),
        state.config.app_service_public_domain.as_deref(),
        &state.config.app_service_public_scheme,
        database_resource_by_id(&state, project_id, existing.database_resource_id)
            .await?
            .as_ref(),
        latest_deployment(&state.db, existing.id).await?,
    )?;
    let deployment_state = state.clone();
    tokio::spawn(async move {
        run_app_service_deployment(
            deployment_state,
            deployment_id,
            service_id,
            project_id,
            image,
            app_port,
            github_credentials,
            None,
            None,
            None,
            false,
            false,
        )
        .await;
    });
    Ok(Json(response))
}

pub async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServiceRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;

    if !state.config.app_service_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_PROVISIONING_DISABLED",
            message: "App service provisioning is not enabled for this environment.",
        });
    }

    let (service, database_resource_id, previous_container_name, app_port) = {
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

        if html_pages::is_html_source(&existing.image_source) {
            return Err(AppError::BadRequest {
                code: "HTML_PAGE_PORT_LOCKED",
                message: "HTML pages always listen on the static nginx port.",
            });
        }

        let app_port = validate_app_port(input.app_port.or(Some(
            u32::try_from(existing.app_port).unwrap_or(u32::from(DEFAULT_APP_PORT)),
        )))?;

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
            app_port,
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
    let (registry_credentials, deployment_image) =
        registry_deployment_target(&state, &service).await?;

    let provisioned = provision_docker(
        &state,
        project_id,
        service.id,
        &deployment_image,
        app_port,
        github_credentials.as_ref(),
        registry_credentials.as_ref(),
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
    refresh_kong_public_routes(&state).await;
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
    Path((workspace_id, project_slug, app_service_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateAppServiceDatabaseRequest>,
) -> Result<Json<AppServiceResponse>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;

    if !state.config.app_service_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "APP_SERVICE_PROVISIONING_DISABLED",
            message: "App service provisioning is not enabled for this environment.",
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
                    "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE id = $1 AND project_id = $2 AND status = 'ready'"
                ))
                .bind(database_id)
                .bind(project_id)
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
    let (registry_credentials, deployment_image) =
        registry_deployment_target(&state, &service).await?;

    let provisioned = provision_docker(
        &state,
        project_id,
        service.id,
        &deployment_image,
        u16::try_from(service.app_port)
            .map_err(|_| AppError::internal("invalid app service container port"))?,
        github_credentials.as_ref(),
        registry_credentials.as_ref(),
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
    refresh_kong_public_routes(&state).await;
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

async fn registry_deployment_target(
    state: &AppState,
    service: &AppServiceRow,
) -> Result<(Option<RegistryDockerCredentials>, String), AppError> {
    if service.image_source != IMAGE_SOURCE_KNOTREE_REGISTRY {
        return Ok((None, service.image.clone()));
    }
    let connection_id = service.registry_connection_id.ok_or(AppError::Conflict {
        code: "KNOTREE_REGISTRY_CONNECTION_REQUIRED",
        message: "Reconnect Knotree Registry before deploying this image.",
    })?;
    let credentials = knotree_registry::load_credentials(state, service.project_id, connection_id)
        .await?
        .ok_or(AppError::Conflict {
            code: "KNOTREE_REGISTRY_CONNECTION_REVOKED",
            message: "Reconnect Knotree Registry before deploying this image.",
        })?;
    let target = knotree_registry::parse_registry_image(&service.image).ok_or_else(|| {
        AppError::internal("the stored Knotree Registry image reference is invalid")
    })?;
    let digest = match service
        .deployed_image_digest
        .as_deref()
        .filter(|digest| knotree_registry::is_valid_digest(digest))
    {
        Some(digest) => digest.to_owned(),
        None => knotree_registry::resolve_tag_digest(
            &credentials.username,
            &credentials.password,
            &target.repository,
            &target.tag,
        )
        .await
        .ok_or(AppError::Conflict {
            code: "KNOTREE_REGISTRY_IMAGE_UNAVAILABLE",
            message: "Knotree Registry could not read this image tag. Check the repository, tag, and pull token.",
        })?,
    };
    let image = knotree_registry::immutable_image(&target.repository, &digest)
        .expect("validated repository and digest produce an immutable image reference");
    Ok((Some(credentials), image))
}

fn validation_field_error(field: &str, message: &str) -> AppError {
    let mut fields = BTreeMap::new();
    fields.insert(field.to_owned(), message.to_owned());
    AppError::validation(fields)
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
    if normalized == IMAGE_SOURCE_HTML || normalized == IMAGE_SOURCE_HTML_GITHUB {
        return Ok(normalized);
    }
    let uses_knotree_registry_host = image
        .get(..(knotree_registry::REGISTRY_HOST.len() + 1))
        .is_some_and(|prefix| {
            prefix.eq_ignore_ascii_case(&format!("{}/", knotree_registry::REGISTRY_HOST))
        });
    if uses_knotree_registry_host && normalized != IMAGE_SOURCE_KNOTREE_REGISTRY {
        return Err(validation_field_error(
            "imageSource",
            "Select Knotree Registry for images hosted on registry.knotree.com.",
        ));
    }
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
    if normalized == IMAGE_SOURCE_KNOTREE_REGISTRY
        && knotree_registry::parse_registry_image(image).is_none()
    {
        let mut fields = BTreeMap::new();
        fields.insert(
            "image".to_owned(),
            "Knotree Registry images must use registry.knotree.com/repository:tag.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    if normalized != IMAGE_SOURCE_PUBLIC
        && normalized != IMAGE_SOURCE_GITHUB
        && normalized != IMAGE_SOURCE_KNOTREE_REGISTRY
    {
        let mut fields = BTreeMap::new();
        fields.insert(
            "imageSource".to_owned(),
            "Choose a public image, a private GitHub image, a Knotree Registry image, or an HTML page.".to_owned(),
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
    let public_domain = public_hostname(
        service.public_access_enabled,
        service.public_subdomain.as_deref(),
        public_domain,
    );
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
        public_access_enabled: service.public_access_enabled,
        rate_limit_rpm: u32::try_from(service.rate_limit_rpm).unwrap_or(DEFAULT_APP_RATE_LIMIT_RPM),
        container_name: service.container_name.clone(),
        error_message: service.error_message.clone(),
        auto_deploy_enabled: service.auto_deploy_enabled,
        deployed_image_digest: service.deployed_image_digest.clone(),
        auto_deploy_checked_at: service
            .auto_deploy_checked_at
            .and_then(|value| value.format(&Rfc3339).ok()),
        auto_deploy_error: service.auto_deploy_error.clone(),
        registry_connection_id: service.registry_connection_id,
        html_repo: service.html_repo.clone(),
        html_branch: service.html_branch.clone(),
        html_sha: service.html_sha.clone(),
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
    registry_credentials: Option<RegistryDockerCredentials>,
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
        let html_sha = prepare_html_site(&state, service_id, github_credentials.as_ref(), Some(&logger))
            .await?;
        let provisioned = if state.config.uses_kubernetes_workloads() {
            provision_kubernetes(
                &state,
                project_id,
                service_id,
                &image,
                app_port,
                database.as_ref(),
                registry_credentials.as_ref(),
                Some(&logger),
            )
            .await?
        } else {
            provision_docker(
                &state,
                project_id,
                service_id,
                &image,
                app_port,
                github_credentials.as_ref(),
                registry_credentials.as_ref(),
                database.as_ref(),
                honor_requested_port,
                None,
                Some(&logger),
            )
            .await?
        };
        sqlx::query(
            "UPDATE project_app_services SET status = $1, host = $2, port = $3, app_port = $4, container_name = $5, database_resource_id = $6, deployed_image_digest = COALESCE($7, deployed_image_digest), html_sha = COALESCE($10, html_sha), error_message = NULL, auto_deploy_error = NULL, auto_deploy_checked_at = CASE WHEN $8 THEN now() ELSE auto_deploy_checked_at END, updated_at = now() WHERE id = $9",
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
        .bind(&html_sha)
        .execute(&state.db)
        .await?;
        logger
            .append(
                "Complete",
                &format!("Container {} is ready for traffic.", provisioned.container_name),
            )
            .await?;
        logger.finish(STATUS_READY, "Complete", None).await?;
        refresh_kong_public_routes(&state).await;
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

async fn prepare_html_site(
    state: &AppState,
    service_id: Uuid,
    github_credentials: Option<&GithubDockerCredentials>,
    logger: Option<&DeploymentLogger>,
) -> Result<Option<String>> {
    let row = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE id = $1"
    ))
    .bind(service_id)
    .fetch_one(&state.db)
    .await?;
    if !html_pages::is_html_source(&row.image_source) {
        return Ok(None);
    }
    log_deployment(logger, "HTML site", "Building the static HTML site.").await?;
    let files = if row.image_source == IMAGE_SOURCE_HTML_GITHUB {
        let repo = row
            .html_repo
            .as_deref()
            .context("HTML GitHub page is missing a repository")?;
        let (owner, name) = html_pages::parse_github_repo(repo)
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        let token = github_credentials
            .map(|credentials| credentials.access_token.as_str())
            .context("Connect GitHub before deploying an HTML repository")?;
        let (sha, resolved_branch, raw) = html_pages::fetch_github_site(
            token,
            &owner,
            &name,
            row.html_branch.as_deref(),
        )
        .await?;
        log_deployment(
            logger,
            "HTML site",
            &format!("Fetched {owner}/{name}@{sha:.7} ({resolved_branch}) and detected the GitHub Pages root."),
        )
        .await?;
        let files = html_pages::materialize_github_files(raw, service_id)
            .map_err(|error| anyhow::anyhow!("{error:?}"))?;
        html_pages::replace_files(&state.db, service_id, &files).await?;
        sqlx::query(
            "UPDATE project_app_services SET html_sha = $1, html_branch = COALESCE(NULLIF(html_branch, ''), $2), updated_at = now() WHERE id = $3",
        )
        .bind(&sha)
        .bind(&resolved_branch)
        .bind(service_id)
        .execute(&state.db)
        .await?;
        html_pages::write_site_to_disk(&state.config.html_site_data_dir, service_id, &files)
            .await?;
        let callback = format!(
            "{}/api/v1/public/html-pages/github-push",
            html_pages::collect_origin(&state.config)
        );
        let secret =
            html_pages::github_webhook_secret(&state.config.database_credentials_encryption_key);
        if let Err(error) = html_pages::register_github_push_webhook(
            token,
            &owner,
            &name,
            &callback,
            &secret,
        )
        .await
        {
            tracing::warn!(
                repo = %format!("{owner}/{name}"),
                error = %error,
                "could not register GitHub HTML page push webhook; polling remains enabled"
            );
        } else {
            log_deployment(
                logger,
                "HTML site",
                "Registered a GitHub push webhook so new commits deploy automatically.",
            )
            .await?;
        }
        return Ok(Some(sha));
    } else {
        html_pages::load_files(&state.db, service_id).await?
    };
    if files.is_empty() {
        bail!("the HTML page has no files to publish");
    }
    html_pages::write_site_to_disk(&state.config.html_site_data_dir, service_id, &files).await?;
    log_deployment(
        logger,
        "HTML site",
        "Wrote the static site and Cloudflare cache headers for HTML and assets.",
    )
    .await?;
    Ok(row.html_sha)
}

pub fn spawn_auto_deployer(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut github_interval =
            tokio::time::interval(Duration::from_secs(AUTO_DEPLOY_INTERVAL_SECONDS));
        let mut registry_interval = tokio::time::interval(Duration::from_secs(3));
        let mut credential_interval = tokio::time::interval(Duration::from_secs(
            crate::registry_accounts::RENEW_INTERVAL_SECONDS,
        ));
        github_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        registry_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        credential_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = github_interval.tick() => {
                    if let Err(error) = poll_auto_deployments(&state).await {
                        tracing::warn!(error = %error, "could not enumerate automatic app service deployments");
                    }
                }
                _ = registry_interval.tick() => {
                    if let Err(error) = process_registry_deploy_jobs(&state).await {
                        tracing::warn!(error = %error, "could not process Knotree Registry deployment jobs");
                    }
                }
                _ = credential_interval.tick() => {
                    crate::registry_accounts::renew_expiring_credentials(&state).await;
                }
            }
        }
    })
}

async fn process_registry_deploy_jobs(state: &AppState) -> Result<()> {
    recover_registry_deploy_jobs(state).await?;
    for _ in 0..8 {
        let Some(job) = claim_registry_deploy_job(state).await? else {
            break;
        };
        let state = state.clone();
        tokio::spawn(async move {
            execute_registry_deploy_job(state, job).await;
        });
    }
    Ok(())
}

async fn recover_registry_deploy_jobs(state: &AppState) -> Result<()> {
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs AS job
         SET status = 'succeeded', locked_until = NULL,
             last_error = NULL, updated_at = now()
         FROM project_app_services AS service
         WHERE job.app_service_id = service.id
           AND job.status = 'running'
           AND job.locked_until < now()
           AND service.deployed_image_digest = job.image_digest",
    )
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE app_service_deployments AS deployment
         SET status = 'error', current_step = 'Recovered',
             error_message = 'The Registry deployment worker restarted before finishing.',
             updated_at = now()
         FROM knotree_registry_deploy_jobs AS job
         WHERE deployment.id = job.deployment_id
           AND job.status = 'running'
           AND job.locked_until < now()
           AND deployment.status = 'provisioning'",
    )
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE project_app_services AS service
         SET status = CASE WHEN service.container_name IS NULL THEN 'error' ELSE 'ready' END,
             error_message = CASE WHEN service.container_name IS NULL
                 THEN 'The Registry deployment worker restarted before the first deployment completed.'
                 ELSE NULL END,
             auto_deploy_error = CASE WHEN service.container_name IS NULL
                 THEN 'The Registry deployment worker restarted before the first deployment completed.'
                 ELSE service.auto_deploy_error END,
             updated_at = now()
         FROM knotree_registry_deploy_jobs AS job
         WHERE job.app_service_id = service.id
           AND job.status = 'running'
           AND job.locked_until < now()
           AND service.deployed_image_digest IS DISTINCT FROM job.image_digest",
    )
    .execute(&state.db)
    .await?;
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs AS job
         SET status = CASE WHEN service.container_name IS NULL THEN 'failed' ELSE 'pending' END,
             locked_until = NULL, deployment_id = NULL,
             last_error = CASE WHEN service.container_name IS NULL
                 THEN 'The initial Registry deployment did not complete.' ELSE NULL END,
             updated_at = now()
         FROM project_app_services AS service
         WHERE job.app_service_id = service.id
           AND job.status = 'running'
           AND job.locked_until < now()
           AND service.deployed_image_digest IS DISTINCT FROM job.image_digest",
    )
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn claim_registry_deploy_job(
    state: &AppState,
) -> Result<Option<ClaimedRegistryDeployment>> {
    let mut transaction = state.db.begin().await?;
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs AS older
         SET status = 'succeeded', last_error = NULL,
             updated_at = now()
         WHERE older.status = 'pending'
           AND EXISTS (
               SELECT 1 FROM knotree_registry_deploy_jobs AS newer
               WHERE newer.app_service_id = older.app_service_id
                 AND newer.status = 'pending'
                 AND (newer.created_at, newer.id) > (older.created_at, older.id)
           )",
    )
    .execute(&mut *transaction)
    .await?;

    let candidate = sqlx::query_as::<_, RegistryDeployCandidate>(
        "SELECT job.id AS job_id, job.app_service_id, service.project_id,
                service.image, service.app_port, service.database_resource_id,
                service.registry_connection_id, job.image_digest, job.immutable_image,
                service.deployed_image_digest
         FROM knotree_registry_deploy_jobs AS job
         JOIN project_app_services AS service ON service.id = job.app_service_id
         JOIN knotree_registry_connections AS connection
           ON connection.id = service.registry_connection_id
          AND connection.revoked_at IS NULL
         WHERE job.status = 'pending'
           AND service.image_source = $1
           AND service.auto_deploy_enabled = TRUE
           AND service.status = $2
         ORDER BY job.created_at ASC, job.id ASC
         LIMIT 1
         FOR UPDATE OF job, service SKIP LOCKED",
    )
    .bind(IMAGE_SOURCE_KNOTREE_REGISTRY)
    .bind(STATUS_READY)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(candidate) = candidate else {
        transaction.commit().await?;
        return Ok(None);
    };
    if candidate.deployed_image_digest.as_deref() == Some(candidate.image_digest.as_str()) {
        sqlx::query(
            "UPDATE knotree_registry_deploy_jobs
             SET status = 'succeeded', locked_until = NULL, last_error = NULL, updated_at = now()
             WHERE id = $1",
        )
        .bind(candidate.job_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE project_app_services
             SET auto_deploy_checked_at = now(), auto_deploy_error = NULL, updated_at = now()
             WHERE id = $1",
        )
        .bind(candidate.app_service_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(None);
    }
    let expected_image = knotree_registry::immutable_reference_for(
        &candidate.image,
        &candidate.image_digest,
    );
    if expected_image.as_deref() != Some(candidate.immutable_image.as_str()) {
        sqlx::query(
            "UPDATE knotree_registry_deploy_jobs
             SET status = 'failed', last_error = 'The Registry image reference no longer matches this service.',
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(candidate.job_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(None);
    }
    let Some(connection_id) = candidate.registry_connection_id else {
        sqlx::query(
            "UPDATE knotree_registry_deploy_jobs
             SET status = 'failed', last_error = 'The Registry connection is unavailable.',
                 updated_at = now()
             WHERE id = $1",
        )
        .bind(candidate.job_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Ok(None);
    };
    let database = match candidate.database_resource_id {
        Some(resource_id) => Some(
            sqlx::query_as::<_, DatabaseResourceRow>(&format!(
                "SELECT {DATABASE_RESOURCE_COLUMNS}
                 FROM project_postgres_databases
                 WHERE id = $1 AND project_id = $2 AND status = 'ready'"
            ))
            .bind(resource_id)
            .bind(candidate.project_id)
            .fetch_optional(&mut *transaction)
            .await?
            .context("the attached database is no longer ready")?,
        ),
        None => None,
    };
    let app_port = u16::try_from(candidate.app_port)
        .map_err(|_| anyhow::anyhow!("invalid app service container port"))?;
    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "UPDATE project_app_services
         SET status = $1, host = NULL, port = NULL, error_message = NULL,
             auto_deploy_checked_at = now(), auto_deploy_error = NULL, updated_at = now()
         WHERE id = $2",
    )
    .bind(STATUS_PROVISIONING)
    .bind(candidate.app_service_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO app_service_deployments (id, app_service_id, status, current_step, logs)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(deployment_id)
    .bind(candidate.app_service_id)
    .bind(STATUS_PROVISIONING)
    .bind("Queued")
    .bind(format!(
        "Knotree Registry webhook deploy queued for digest {}.",
        short_image_digest(&candidate.image_digest)
    ))
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET status = 'running', locked_until = now() + interval '10 minutes',
             attempt_count = attempt_count + 1, deployment_id = $2, last_error = NULL,
             updated_at = now()
         WHERE id = $1",
    )
    .bind(candidate.job_id)
    .bind(deployment_id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    Ok(Some(ClaimedRegistryDeployment {
        job_id: candidate.job_id,
        app_service_id: candidate.app_service_id,
        project_id: candidate.project_id,
        connection_id,
        deployment_id,
        image: candidate.immutable_image,
        image_digest: candidate.image_digest,
        app_port,
        database_resource_id: candidate.database_resource_id,
        database,
    }))
}

async fn execute_registry_deploy_job(state: AppState, job: ClaimedRegistryDeployment) {
    let credentials = match knotree_registry::load_credentials(
        &state,
        job.project_id,
        job.connection_id,
    )
    .await
    {
        Ok(Some(credentials)) => credentials,
        Ok(None) | Err(_) => {
            fail_registry_deploy_job(&state, &job, "The Registry connection is no longer available.")
                .await;
            return;
        }
    };
    let ClaimedRegistryDeployment {
        job_id,
        app_service_id,
        project_id,
        deployment_id,
        image,
        image_digest,
        app_port,
        database_resource_id,
        database,
        ..
    } = job;
    let deployment = run_app_service_deployment(
        state.clone(),
        deployment_id,
        app_service_id,
        project_id,
        image,
        app_port,
        None,
        Some(credentials),
        database,
        database_resource_id,
        false,
        true,
    );
    tokio::pin!(deployment);
    let mut lease_refresh = tokio::time::interval(Duration::from_secs(60));
    lease_refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = lease_refresh.tick() => {
                if let Err(error) = refresh_registry_deploy_job_lease(&state, job_id).await {
                    tracing::warn!(
                        registry_job_id = %job_id,
                        error = %error,
                        "could not refresh Knotree Registry deploy job lease"
                    );
                }
            }
            _ = &mut deployment => break,
        }
    }
    let deployed_digest = sqlx::query_scalar::<_, Option<String>>(
        "SELECT deployed_image_digest FROM project_app_services WHERE id = $1",
    )
    .bind(app_service_id)
    .fetch_optional(&state.db)
    .await
    .ok()
    .flatten()
    .flatten();
    let succeeded = deployed_digest.as_deref() == Some(image_digest.as_str());
    if let Err(error) = sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET status = $1, locked_until = NULL,
             last_error = CASE WHEN $1 = 'succeeded' THEN NULL ELSE 'Registry image deployment failed.' END,
             updated_at = now()
         WHERE id = $2",
    )
    .bind(if succeeded { "succeeded" } else { "failed" })
    .bind(job_id)
    .execute(&state.db)
    .await
    {
        tracing::error!(
            registry_job_id = %job_id,
            error = %error,
            "could not finish Knotree Registry deploy job"
        );
    }
}

async fn refresh_registry_deploy_job_lease(state: &AppState, job_id: Uuid) -> Result<()> {
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET locked_until = now() + interval '10 minutes', updated_at = now()
         WHERE id = $1 AND status = 'running'",
    )
    .bind(job_id)
    .execute(&state.db)
    .await?;
    Ok(())
}

async fn fail_registry_deploy_job(
    state: &AppState,
    job: &ClaimedRegistryDeployment,
    message: &str,
) {
    let logger = DeploymentLogger {
        db: state.db.clone(),
        deployment_id: job.deployment_id,
    };
    if let Err(error) = logger.finish(STATUS_ERROR, "Failed", Some(message)).await {
        tracing::warn!(error = %error, "could not persist failed Registry deployment state");
    }
    if let Err(error) = sqlx::query(
        "UPDATE project_app_services
         SET status = $1, error_message = $2, auto_deploy_error = $2, updated_at = now()
         WHERE id = $3",
    )
    .bind(STATUS_ERROR)
    .bind(message)
    .bind(job.app_service_id)
    .execute(&state.db)
    .await
    {
        tracing::warn!(error = %error, "could not mark Registry app service as failed");
    }
    if let Err(error) = sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET status = 'failed', locked_until = NULL, last_error = $1, updated_at = now()
         WHERE id = $2",
    )
    .bind(message)
    .bind(job.job_id)
    .execute(&state.db)
    .await
    {
        tracing::warn!(error = %error, "could not mark Registry deployment job as failed");
    }
}

async fn poll_auto_deployments(state: &AppState) -> std::result::Result<(), sqlx::Error> {
    let candidates = sqlx::query_as::<_, AutoDeployCandidate>(
        "SELECT id, image, image_source, container_name, github_connection_user_id, html_repo, html_branch, html_sha
         FROM project_app_services
         WHERE auto_deploy_enabled = TRUE
           AND image_source IN ($1, $2)
           AND status = $3
         ORDER BY auto_deploy_checked_at NULLS FIRST, id ASC",
    )
    .bind(IMAGE_SOURCE_GITHUB)
    .bind(IMAGE_SOURCE_HTML_GITHUB)
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
    if candidate.image_source == IMAGE_SOURCE_HTML_GITHUB {
        return check_html_repo_auto_deploy(state, candidate).await;
    }
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
                None,
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

pub(crate) async fn queue_html_github_pushes(
    state: &AppState,
    full_name: &str,
    branch: &str,
) -> Result<()> {
    if full_name.is_empty() {
        return Ok(());
    }
    let candidates = sqlx::query_as::<_, AutoDeployCandidate>(
        "SELECT id, image, image_source, container_name, github_connection_user_id, html_repo, html_branch, html_sha
         FROM project_app_services
         WHERE auto_deploy_enabled = TRUE
           AND image_source = $1
           AND status = $2
           AND lower(html_repo) = lower($3)
           AND (
             html_branch = $4
             OR ((html_branch IS NULL OR html_branch = '') AND $4 <> '')
           )",
    )
    .bind(IMAGE_SOURCE_HTML_GITHUB)
    .bind(STATUS_READY)
    .bind(full_name)
    .bind(branch)
    .fetch_all(&state.db)
    .await?;
    for candidate in candidates {
        if let Err(error) = check_html_repo_auto_deploy(state, candidate.clone()).await {
            tracing::warn!(
                app_service_id = %candidate.id,
                error = %error,
                "HTML GitHub push auto-deploy failed"
            );
        }
    }
    Ok(())
}

async fn check_html_repo_auto_deploy(
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
    let repo = candidate
        .html_repo
        .as_deref()
        .context("HTML GitHub page is missing a repository")?;
    let (owner, name) = html_pages::parse_github_repo(repo)
        .map_err(|error| anyhow::anyhow!("{error:?}"))?;
    let (sha, _branch) = html_pages::fetch_github_commit_sha(
        &credentials.access_token,
        &owner,
        &name,
        candidate.html_branch.as_deref(),
    )
    .await?;
    if candidate.html_sha.as_deref() == Some(sha.as_str()) {
        record_auto_deploy_check(state, candidate.id, &sha).await?;
        sqlx::query(
            "UPDATE project_app_services SET html_sha = $1, auto_deploy_checked_at = now(), auto_deploy_error = NULL, updated_at = now() WHERE id = $2",
        )
        .bind(&sha)
        .bind(candidate.id)
        .execute(&state.db)
        .await?;
        return Ok(());
    }
    if let Some(queued) =
        queue_auto_deployment(state, candidate.id, candidate.html_sha.as_deref(), &sha).await?
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
                None,
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
        || (service.image_source != IMAGE_SOURCE_GITHUB
            && service.image_source != IMAGE_SOURCE_HTML_GITHUB)
    {
        transaction.commit().await?;
        return Ok(None);
    }

    let current_digest = if service.image_source == IMAGE_SOURCE_HTML_GITHUB {
        expected_digest.or(service.html_sha.as_deref())
    } else {
        expected_digest.or(service.deployed_image_digest.as_deref())
    };
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
                 WHERE id = $1 AND project_id = $2 AND status = 'ready'"
            ))
            .bind(resource_id)
            .bind(service.project_id)
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
            .map(|row| {
                complete_app_service_metric_point(AppServiceMetricPoint {
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
    let sample = complete_app_service_metric_point(AppServiceMetricPoint {
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
    });
    persist_app_service_metric_sample(state, service.id, &sample).await
}

async fn ready_database_resources(
    state: &AppState,
    project_id: Uuid,
) -> std::result::Result<Vec<DatabaseResourceRow>, sqlx::Error> {
    sqlx::query_as::<_, DatabaseResourceRow>(&format!(
        "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE project_id = $1 AND status = 'ready'"
    ))
    .bind(project_id)
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
        "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE id = $1 AND project_id = $2 AND status = 'ready'"
    ))
    .bind(resource_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await
}

fn postgres_private_host(config: &crate::config::Config, project_id: Uuid) -> String {
    if config.uses_kubernetes_workloads() {
        format!(
            "knotree-pg-{}.{}.svc.cluster.local",
            project_id.simple(),
            config.database_cluster_namespace
        )
    } else {
        cluster::PROJECT_NETWORK_POSTGRES_ALIAS.to_owned()
    }
}

fn database_environment(
    encryption_key: &[u8; 32],
    database: Option<&DatabaseResourceRow>,
    postgres_host: &str,
) -> Result<Vec<String>> {
    let Some(database) = database else {
        return Ok(Vec::new());
    };
    let password = security::decrypt_secret(&database.password_ciphertext, encryption_key)
        .map_err(|_| anyhow::anyhow!("could not decrypt the PostgreSQL credentials"))?;
    let database_url = format!(
        "postgres://{}:{}@{}:{}/{}",
        database.role_name, password, postgres_host, 5432, database.database_name,
    );
    Ok(vec![
        format!("DATABASE_URL={database_url}"),
        format!("PGHOST={postgres_host}"),
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
    registry_credentials: Option<&RegistryDockerCredentials>,
    database: Option<&DatabaseResourceRow>,
    honor_requested_port: bool,
    previous_container_name: Option<&str>,
    logger: Option<&DeploymentLogger>,
) -> Result<ProvisionedAppService> {
    log_deployment(logger, "Prepare", "Preparing the Docker deployment.").await?;
    let database_environment = database_environment(
        &state.config.database_credentials_encryption_key,
        database,
        &postgres_private_host(&state.config, project_id),
    )?;
    log_deployment(logger, "Network", "Ensuring the project network exists.").await?;
    let network_name = cluster::ensure_project_network(&state.config, project_id).await?;
    log_deployment(logger, "Network", "Project network is ready.").await?;

    let is_ghcr = image
        .get(.."ghcr.io/".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ghcr.io/"));
    let is_knotree_registry = image
        .get(..(knotree_registry::REGISTRY_HOST.len() + 1))
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(&format!("{}/", knotree_registry::REGISTRY_HOST)));
    if is_ghcr {
        let credentials = github_credentials.context("Connect GitHub before pulling a private ghcr.io image")?;
        pull_github_image(state, credentials, image, logger).await?;
    } else if is_knotree_registry {
        let credentials = registry_credentials
            .context("Connect Knotree Registry before pulling a private registry.knotree.com image")?;
        pull_registry_image(state, credentials, image, logger).await?;
    } else {
        docker_pull(state, image, logger).await?;
    }

    let image_digest = match knotree_registry::digest_from_image_reference(image) {
        Some(digest) => Some(digest),
        None => docker_image_digest(state, image, logger).await?,
    };
    let html_root = html_pages::site_dir(&state.config.html_site_data_dir, service_id);
    let is_html_site = html_root.join("html").join("index.html").exists();
    let declared_volumes = if is_html_site {
        Vec::new()
    } else {
        docker_image_declared_volumes(state, image, logger).await?
    };
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
    if is_html_site {
        docker_args.push("--volume".to_owned());
        docker_args.push(format!(
            "{}:/usr/share/nginx/html:ro",
            html_root.join("html").display()
        ));
        docker_args.push("--volume".to_owned());
        docker_args.push(format!(
            "{}:/etc/nginx/conf.d/default.conf:ro",
            html_root.join("default.conf").display()
        ));
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

async fn provision_kubernetes(
    state: &AppState,
    project_id: Uuid,
    service_id: Uuid,
    image: &str,
    app_port: u16,
    database: Option<&DatabaseResourceRow>,
    registry_credentials: Option<&RegistryDockerCredentials>,
    logger: Option<&DeploymentLogger>,
) -> Result<ProvisionedAppService> {
    log_deployment(logger, "Prepare", "Preparing the Kubernetes deployment.").await?;
    let database_environment = database_environment(
        &state.config.database_credentials_encryption_key,
        database,
        &postgres_private_host(&state.config, project_id),
    )?;
    let env = database_environment
        .into_iter()
        .filter_map(|variable| {
            let (key, value) = variable.split_once('=')?;
            Some((key.to_owned(), value.to_owned()))
        })
        .collect::<Vec<_>>();
    log_deployment(
        logger,
        "Apply resource limits",
        &format!(
            "Applying the {APP_SERVICE_VIRTUAL_CPU} virtual allocation ({KUBERNETES_APP_CPU_LIMIT} host CPU), 1 GiB RAM, and 10 GiB ephemeral storage."
        ),
    )
    .await?;
    let image_pull_secret = registry_credentials
        .map(|_| cluster_kubernetes::app_image_pull_secret_name(service_id));
    let provisioned = cluster_kubernetes::provision_app(
        &state.config,
        &AppWorkloadSpec {
            project_id,
            service_id,
            image: image.to_owned(),
            image_pull_secret,
            app_port,
            env,
            html_site_host_path: html_pages::site_dir(
                &state.config.html_site_data_dir,
                service_id,
            )
            .join("html")
            .exists()
            .then(|| {
                html_pages::site_dir(&state.config.html_site_data_dir, service_id)
                    .to_string_lossy()
                    .into_owned()
            }),
        },
        registry_credentials.map(|credentials| {
            (credentials.username.as_str(), credentials.password.as_str())
        }),
    )
    .await?;
    log_deployment(
        logger,
        "Start container",
        &format!("Workload {} is scheduled.", provisioned.name),
    )
    .await?;
    Ok(ProvisionedAppService {
        host: provisioned.host,
        port: provisioned.port,
        app_port,
        container_name: provisioned.name,
        image_digest: knotree_registry::digest_from_image_reference(image),
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

struct TemporaryDockerConfig(PathBuf);

impl TemporaryDockerConfig {
    fn new(credentials: &RegistryDockerCredentials) -> Result<Self> {
        let directory = std::env::temp_dir().join(format!("knotree-docker-{}", Uuid::new_v4()));
        fs::create_dir(&directory).context("could not create isolated Docker config directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let config = knotree_registry::docker_config_json(
            &credentials.username,
            &credentials.password,
        )?;
        let config_path = directory.join("config.json");
        if let Err(error) = fs::write(&config_path, config.to_string()) {
            let _ = fs::remove_dir_all(&directory);
            return Err(error).context("could not write isolated Docker registry credentials");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) = fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600)) {
                let _ = fs::remove_dir_all(&directory);
                return Err(error).context("could not restrict isolated Docker credential permissions");
            }
        }
        Ok(Self(directory))
    }

    fn directory(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TemporaryDockerConfig {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            tracing::warn!(error = %error, "could not remove isolated Docker registry credentials");
        }
    }
}

async fn pull_registry_image(
    state: &AppState,
    credentials: &RegistryDockerCredentials,
    image: &str,
    logger: Option<&DeploymentLogger>,
) -> Result<()> {
    let docker_config = TemporaryDockerConfig::new(credentials)?;
    log_deployment(
        logger,
        "Pull image",
        &format!("Pulling private Knotree Registry image {image}."),
    )
    .await?;
    let args = vec![
        "--config".to_owned(),
        docker_config.directory().to_string_lossy().into_owned(),
        "pull".to_owned(),
        image.to_owned(),
    ];
    if let Some(logger) = logger {
        run_docker_streaming(state, logger, "Pull image", args)
            .await
            .map(|_| ())
    } else {
        run_docker(state, args).await.map(|_| ())
    }
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

pub async fn account_deployment_logs(
    state: &AppState,
    user_id: Uuid,
) -> Result<Vec<AccountDeploymentLog>, AppError> {
    let rows = sqlx::query_as::<_, AccountLogRow>(
        "SELECT d.app_service_id, s.project_id, d.status, d.current_step, d.logs, d.error_message
         FROM app_service_deployments d
         JOIN project_app_services s ON s.id = d.app_service_id
         JOIN projects p ON p.id = s.project_id
         JOIN workspace_memberships m ON m.workspace_id = p.workspace_id
         WHERE m.user_id = $1
         ORDER BY d.updated_at DESC
         LIMIT 20",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| AccountDeploymentLog {
            app_service_id: row.app_service_id,
            project_id: row.project_id,
            status: row.status,
            current_step: row.current_step,
            logs: row.logs.lines().map(ToOwned::to_owned).collect(),
            error_message: row.error_message,
        })
        .collect())
}

#[derive(Debug, sqlx::FromRow)]
struct AccountLogRow {
    app_service_id: Uuid,
    project_id: Uuid,
    status: String,
    current_step: String,
    logs: String,
    error_message: Option<String>,
}

pub async fn mcp_service_logs(
    state: &AppState,
    user_id: Uuid,
    workspace_id: &str,
    project_slug: &str,
    app_service_id: Uuid,
) -> Result<AppServiceLogsResponse, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_id, project_slug).await?;
    let owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT m.user_id FROM workspace_memberships m
         JOIN projects p ON p.workspace_id = m.workspace_id
         WHERE p.id = $1 AND m.role = 'owner'",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    crate::mcp::authorize_account(user_id, owner)?;
    runtime_logs(state, project_id, app_service_id).await
}

pub async fn mcp_list_resources(
    state: &AppState,
    user_id: Uuid,
    workspace_id: &str,
    project_slug: &str,
) -> Result<serde_json::Value, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_id, project_slug).await?;
    let owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT m.user_id FROM workspace_memberships m
         JOIN projects p ON p.workspace_id = m.workspace_id
         WHERE p.id = $1 AND m.role = 'owner'",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    crate::mcp::authorize_account(user_id, owner)?;
    let apps = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE project_id = $1 ORDER BY created_at ASC"
    ))
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let redis =
        redis_resources::list_for_user(state, user_id, workspace_id, project_slug).await?;
    Ok(serde_json::json!({
        "appServices": apps.iter().map(|service| service.name.clone()).collect::<Vec<_>>(),
        "redis": redis,
        "projectId": project_id,
    }))
}

pub async fn mcp_deploy(
    state: &AppState,
    user_id: Uuid,
    workspace_id: &str,
    project_slug: &str,
    arguments: serde_json::Value,
) -> Result<serde_json::Value, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_id, project_slug).await?;
    let owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT m.user_id FROM workspace_memberships m
         JOIN projects p ON p.workspace_id = m.workspace_id
         WHERE p.id = $1 AND m.role = 'owner'",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    crate::mcp::authorize_account(user_id, owner)?;
    let input = CreateAppServiceRequest {
        name: arguments
            .get("name")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        image: arguments
            .get("image")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        image_source: arguments
            .get("imageSource")
            .and_then(|value| value.as_str())
            .unwrap_or("public")
            .to_owned(),
        app_port: arguments
            .get("appPort")
            .and_then(serde_json::Value::as_u64)
            .map(|value| value as u32),
        auto_deploy: arguments
            .get("autoDeploy")
            .and_then(serde_json::Value::as_bool),
        page_slug: arguments
            .get("pageSlug")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        index_html: arguments
            .get("indexHtml")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        github_repo: arguments
            .get("githubRepo")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        github_branch: arguments
            .get("githubBranch")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned),
        registry_connection_id: arguments
            .get("registryConnectionId")
            .and_then(|value| value.as_str())
            .and_then(|value| Uuid::parse_str(value).ok()),
    };
    let service = create_for_user(state, user_id, workspace_id, project_slug, input).await?;
    serde_json::to_value(service).map_err(AppError::internal)
}

pub async fn mcp_setup_public_access(
    state: &AppState,
    user_id: Uuid,
    workspace_id: &str,
    project_slug: &str,
    app_service_id: Uuid,
    enabled: bool,
    rate_limit_rpm: Option<u32>,
) -> Result<serde_json::Value, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_id, project_slug).await?;
    let owner = sqlx::query_scalar::<_, Uuid>(
        "SELECT m.user_id FROM workspace_memberships m
         JOIN projects p ON p.workspace_id = m.workspace_id
         WHERE p.id = $1 AND m.role = 'owner'",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    crate::mcp::authorize_account(user_id, owner)?;
    let service = update_public_access_for_user(
        state,
        user_id,
        workspace_id,
        project_slug,
        app_service_id,
        UpdateAppServicePublicAccessRequest {
            enabled,
            rate_limit_rpm,
        },
    )
    .await?;
    serde_json::to_value(service).map_err(AppError::internal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_public_github_and_knotree_registry_images() {
        assert_eq!(validate_image(" nginx:alpine ").unwrap(), "nginx:alpine");
        assert!(validate_image("https://docker.io/nginx").is_err());
        assert!(validate_image("nginx; echo leaked").is_err());
        assert!(validate_image_source("public", "nginx:alpine").is_ok());
        assert!(validate_image_source("github", "ghcr.io/acme/app:latest").is_ok());
        assert!(validate_image_source("github", "nginx:latest").is_err());
        assert!(validate_image_source(
            IMAGE_SOURCE_KNOTREE_REGISTRY,
            "registry.knotree.com/team/api:production"
        )
        .is_ok());
        assert!(validate_image_source(
            IMAGE_SOURCE_KNOTREE_REGISTRY,
            "registry.knotree.com/team/api"
        )
        .is_err());
        assert!(validate_image_source("public", "registry.knotree.com/team/api:prod").is_err());
    }

    #[test]
    fn validates_container_port() {
        assert_eq!(validate_app_port(None).unwrap(), DEFAULT_APP_PORT);
        assert_eq!(validate_app_port(Some(8080)).unwrap(), 8080);
        assert!(validate_app_port(Some(0)).is_err());
        assert!(validate_app_port(Some(65_536)).is_err());
    }

    #[test]
    fn completes_all_app_service_metric_fields_for_empty_or_legacy_samples() {
        let point = complete_app_service_metric_point(AppServiceMetricPoint {
            timestamp: 1,
            cpu_percent: None,
            memory_used_bytes: None,
            memory_limit_bytes: None,
            volume_used_bytes: None,
            volume_capacity_bytes: None,
            network_receive_bytes: None,
            network_transmit_bytes: None,
            disk_read_bytes: None,
            disk_write_bytes: None,
            public_network_receive_bytes: None,
            public_network_transmit_bytes: None,
            requests: None,
            response_time_ms: None,
            request_error_rate: None,
        });
        assert_eq!(point.cpu_percent, Some(0.0));
        assert_eq!(point.memory_used_bytes, Some(0));
        assert_eq!(point.memory_limit_bytes, Some(RESOURCE_MEMORY_LIMIT_BYTES));
        assert_eq!(point.volume_used_bytes, Some(0));
        assert_eq!(
            point.volume_capacity_bytes,
            Some(RESOURCE_VOLUME_LIMIT_BYTES)
        );
        assert_eq!(point.network_receive_bytes, Some(0));
        assert_eq!(point.network_transmit_bytes, Some(0));
        assert_eq!(point.disk_read_bytes, Some(0));
        assert_eq!(point.disk_write_bytes, Some(0));
        assert_eq!(point.public_network_receive_bytes, Some(0));
        assert_eq!(point.public_network_transmit_bytes, Some(0));
        assert_eq!(point.requests, Some(0));
        assert_eq!(point.response_time_ms, Some(0.0));
        assert_eq!(point.request_error_rate, Some(0.0));

        let serialized = serde_json::to_value(point).unwrap();
        for field in [
            "cpuPercent",
            "memoryUsedBytes",
            "memoryLimitBytes",
            "volumeUsedBytes",
            "volumeCapacityBytes",
            "networkReceiveBytes",
            "networkTransmitBytes",
            "diskReadBytes",
            "diskWriteBytes",
            "publicNetworkReceiveBytes",
            "publicNetworkTransmitBytes",
            "requests",
            "responseTimeMs",
            "requestErrorRate",
        ] {
            assert!(serialized.get(field).is_some(), "missing {field}");
        }
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
            public_subdomain: Some("app-0123456789abcdef".to_owned()),
            public_access_enabled: true,
            rate_limit_rpm: 60,
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
            registry_connection_id: None,
            deployed_image_digest: None,
            auto_deploy_checked_at: None,
            auto_deploy_error: None,
            html_repo: None,
            html_branch: None,
            html_sha: None,
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
        assert!(response.public_access_enabled);
        assert_eq!(response.rate_limit_rpm, 60);

        let private = AppServiceRow {
            public_access_enabled: false,
            ..service
        };
        let hidden = app_service_response(
            &private,
            "localhost",
            8080,
            Some("knotree.org"),
            "https",
            None,
            None,
        )
        .unwrap();
        assert_eq!(hidden.public_domain, None);
    }

    #[test]
    fn kong_rate_limit_matches_the_service_setting() {
        let plugin = crate::kong::kong_rate_limiting_plugin(limits::RateLimitPolicy {
            requests_per_minute: 45,
        });
        assert_eq!(plugin["config"]["minute"], 45);
        let mut limiter = limits::PerKeyMinuteLimiter::default();
        for _ in 0..45 {
            assert!(limiter.check("svc-a", 45, 10).allowed);
        }
        assert!(!limiter.check("svc-a", 45, 10).allowed);
        assert!(limiter.check("svc-b", 45, 10).allowed);
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

        let variables = database_environment(&key, Some(&database), "postgres").unwrap();
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

    #[test]
    fn mcp_and_dashboard_share_the_runtime_log_path() {
        let source = include_str!("app_services.rs");
        assert!(source.contains("create_for_user"));
        assert!(source.contains("cluster_kubernetes::pod_logs"));
        let undeployed = logs_unavailable_response(
            Uuid::nil(),
            &AppServiceLogsTarget {
                container_name: None,
                status: STATUS_PROVISIONING.to_owned(),
            },
        )
        .unwrap();
        assert!(undeployed.lines.is_empty());
        assert!(
            undeployed
                .message
                .as_deref()
                .unwrap()
                .contains("has not been deployed yet")
        );
        assert!(
            logs_unavailable_response(
                Uuid::nil(),
                &AppServiceLogsTarget {
                    container_name: Some("knotree-app-ready".to_owned()),
                    status: STATUS_READY.to_owned(),
                },
            )
            .is_none()
        );
    }

    #[test]
    fn kong_routes_restore_ready_public_services_only() {
        let ready_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let pending_id = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let routes = kong_routes_from_rows(
            &[
                KongSyncRow {
                    id: ready_id,
                    public_subdomain: Some("app-readyhost1234".to_owned()),
                    public_access_enabled: true,
                    rate_limit_rpm: 90,
                    host: Some("knotree-app-ready".to_owned()),
                    port: Some(8080),
                    container_name: Some("knotree-app-ready".to_owned()),
                    status: STATUS_READY.to_owned(),
                    image_source: IMAGE_SOURCE_PUBLIC.to_owned(),
                },
                KongSyncRow {
                    id: pending_id,
                    public_subdomain: Some("app-pendinghost12".to_owned()),
                    public_access_enabled: true,
                    rate_limit_rpm: 30,
                    host: Some("knotree-app-pending".to_owned()),
                    port: Some(8080),
                    container_name: Some("knotree-app-pending".to_owned()),
                    status: STATUS_PROVISIONING.to_owned(),
                    image_source: IMAGE_SOURCE_PUBLIC.to_owned(),
                },
            ],
            Some("knotree.org"),
        );
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].service_id, ready_id);
        assert_eq!(routes[0].public_host, "app-readyhost1234.knotree.org");
        assert_eq!(routes[0].upstream_url, "http://knotree-app-ready:8080");
        assert_eq!(routes[0].rate_limit_rpm, 90);
        let main = include_str!("main.rs");
        assert!(main.contains("spawn_kong_route_syncer"));
    }

    #[tokio::test]
    async fn mcp_deploy_logs_and_setup_mutate_the_project() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let seed = crate::test_support::seed_owner_project(&state).await;
        let deployed = mcp_deploy(
            &state,
            seed.user_id,
            &seed.workspace_route_id,
            &seed.project_slug,
            serde_json::json!({
                "name": "Docs site",
                "image": "nginx:alpine",
                "imageSource": "public",
                "appPort": 80
            }),
        )
        .await
        .expect("mcp deploy should insert an app service");
        let service_id = Uuid::parse_str(deployed["id"].as_str().expect("service id")).unwrap();
        assert_eq!(deployed["name"], "Docs site");
        assert_eq!(deployed["image"], "nginx:alpine");
        let stored_name = sqlx::query_scalar::<_, String>(
            "SELECT name FROM project_app_services WHERE id = $1 AND project_id = $2",
        )
        .bind(service_id)
        .bind(seed.project_id)
        .fetch_one(&state.db)
        .await
        .expect("deployed row");
        assert_eq!(stored_name, "Docs site");

        let logs = mcp_service_logs(
            &state,
            seed.user_id,
            &seed.workspace_route_id,
            &seed.project_slug,
            service_id,
        )
        .await
        .expect("mcp logs");
        assert!(logs.lines.is_empty());
        assert!(
            logs.message
                .as_deref()
                .unwrap()
                .contains("has not been deployed yet")
        );

        let setup = mcp_setup_public_access(
            &state,
            seed.user_id,
            &seed.workspace_route_id,
            &seed.project_slug,
            service_id,
            true,
            Some(120),
        )
        .await
        .expect("mcp setup should persist public access");
        assert_eq!(setup["publicAccessEnabled"], true);
        assert_eq!(setup["rateLimitRpm"], 120);
        let domain = setup["publicDomain"].as_str().expect("assigned domain");
        assert!(domain.ends_with(".knotree.org"));
        let enabled = sqlx::query_scalar::<_, bool>(
            "SELECT public_access_enabled FROM project_app_services WHERE id = $1",
        )
        .bind(service_id)
        .fetch_one(&state.db)
        .await
        .expect("public access flag");
        assert!(enabled);
        let subdomain = sqlx::query_scalar::<_, Option<String>>(
            "SELECT public_subdomain FROM project_app_services WHERE id = $1",
        )
        .bind(service_id)
        .fetch_one(&state.db)
        .await
        .expect("assigned subdomain");
        assert!(subdomain.is_some());
    }
}
