use std::{
    collections::BTreeMap,
    process::{Output, Stdio},
};

use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::{io::AsyncWriteExt, process::Command};
use uuid::Uuid;

use crate::{
    auth,
    cluster::PROVIDER_DOCKER,
    error::AppError,
    github::{self, GithubDockerCredentials},
    models::{AppServiceResponse, CreateAppServiceRequest},
    projects, security,
    state::AppState,
};

const IMAGE_SOURCE_PUBLIC: &str = "public";
const IMAGE_SOURCE_GITHUB: &str = "github";
const STATUS_READY: &str = "ready";
const STATUS_PROVISIONING: &str = "provisioning";
const STATUS_ERROR: &str = "error";
const DEFAULT_APP_PORT: u16 = 3000;
const PROVISIONING_ERROR_MESSAGE: &str =
    "The Docker app service could not be deployed. Check the image and try again.";

#[derive(Debug, Clone, sqlx::FromRow)]
struct AppServiceRow {
    id: Uuid,
    name: String,
    image: String,
    image_source: String,
    app_port: i32,
    host: Option<String>,
    port: Option<i32>,
    container_name: Option<String>,
    status: String,
    error_message: Option<String>,
}

const APP_SERVICE_COLUMNS: &str =
    "id, name, image, image_source, app_port, host, port, container_name, status, error_message";

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<AppServiceResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE project_id = $1"
    ))
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    service
        .map(|service| app_service_response(&service, &state.config.app_service_public_host))
        .transpose()
        .map(|service| Json(service.into_iter().collect()))
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

    let (service, is_new) = {
        let mut transaction = state.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *transaction)
            .await?;

        let existing = sqlx::query_as::<_, AppServiceRow>(&format!(
            "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE project_id = $1 FOR UPDATE"
        ))
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?;

        let result = match existing {
            Some(service) if service.status == STATUS_READY => (service, false),
            Some(service) if service.status == STATUS_PROVISIONING => {
                return Err(AppError::Conflict {
                    code: "APP_SERVICE_PROVISIONING",
                    message: "This app service is already being deployed.",
                });
            }
            Some(service) => {
                let service = sqlx::query_as::<_, AppServiceRow>(&format!(
                    "UPDATE project_app_services SET name = $1, image = $2, image_source = $3, app_port = $4, host = NULL, port = NULL, status = $5, error_message = NULL, updated_at = now() WHERE id = $6 RETURNING {APP_SERVICE_COLUMNS}"
                ))
                .bind(&name)
                .bind(&image)
                .bind(&image_source)
                .bind(i32::from(app_port))
                .bind(STATUS_PROVISIONING)
                .bind(service.id)
                .fetch_one(&mut *transaction)
                .await?;
                (service, false)
            }
            None => {
                let service = sqlx::query_as::<_, AppServiceRow>(&format!(
                    "INSERT INTO project_app_services (id, project_id, name, image, image_source, app_port, status) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING {APP_SERVICE_COLUMNS}"
                ))
                .bind(Uuid::new_v4())
                .bind(project_id)
                .bind(&name)
                .bind(&image)
                .bind(&image_source)
                .bind(i32::from(app_port))
                .bind(STATUS_PROVISIONING)
                .fetch_one(&mut *transaction)
                .await?;
                (service, true)
            }
        };
        transaction.commit().await?;
        result
    };

    if service.status == STATUS_READY {
        return Ok((
            StatusCode::OK,
            Json(app_service_response(
                &service,
                &state.config.app_service_public_host,
            )?),
        )
            .into_response());
    }

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

    let provisioned = provision_docker(
        &state,
        project_id,
        &image,
        app_port,
        github_credentials.as_ref(),
    )
    .await;
    let provisioned = match provisioned {
        Ok(provisioned) => provisioned,
        Err(error) => {
            tracing::error!(
                project_id = %project_id,
                service_id = %service.id,
                error = %error,
                "app service provisioning failed"
            );
            sqlx::query(
                "UPDATE project_app_services SET status = $1, error_message = $2, updated_at = now() WHERE id = $3",
            )
            .bind(STATUS_ERROR)
            .bind(PROVISIONING_ERROR_MESSAGE)
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
        "UPDATE project_app_services SET status = $1, host = $2, port = $3, container_name = $4, error_message = NULL, updated_at = now() WHERE id = $5 RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(STATUS_READY)
    .bind(&provisioned.host)
    .bind(i32::from(provisioned.port))
    .bind(&provisioned.container_name)
    .bind(service.id)
    .fetch_one(&state.db)
    .await?;
    let response = app_service_response(&service, &state.config.app_service_public_host)?;
    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(response)).into_response())
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
    let service_url = match (host.as_deref(), port) {
        (Some(host), Some(port)) => Some(format!("http://{host}:{port}")),
        _ => None,
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
        container_name: service.container_name.clone(),
        error_message: service.error_message.clone(),
    })
}

#[derive(Debug)]
struct ProvisionedAppService {
    host: String,
    port: u16,
    container_name: String,
}

async fn provision_docker(
    state: &AppState,
    project_id: Uuid,
    image: &str,
    app_port: u16,
    github_credentials: Option<&GithubDockerCredentials>,
) -> Result<ProvisionedAppService> {
    if let Some(credentials) = github_credentials {
        if !image
            .get(.."ghcr.io/".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("ghcr.io/"))
        {
            bail!("private app images must be hosted on ghcr.io");
        }
        docker_login(state, credentials).await?;
        let pull_result = docker_pull(state, image).await;
        let _ = docker_logout(state).await;
        pull_result?;
    } else {
        docker_pull(state, image).await?;
    }

    let container_name = format!("knotree-app-{}", project_id.simple());
    remove_existing_container(state, &container_name).await?;
    let publish = format!("{}::{}", state.config.app_service_bind_address, app_port);
    run_docker(
        state,
        [
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
            "--publish".to_owned(),
            publish,
            image.to_owned(),
        ],
    )
    .await?;

    let running = docker_inspect_running(state, &container_name).await?;
    if running != "true" {
        let _ = remove_existing_container(state, &container_name).await;
        bail!("Docker app container exited during startup");
    }
    let port = match docker_port(state, &container_name, app_port).await {
        Ok(port) => port,
        Err(error) => {
            let _ = remove_existing_container(state, &container_name).await;
            return Err(error);
        }
    };

    Ok(ProvisionedAppService {
        host: state.config.app_service_public_host.clone(),
        port,
        container_name,
    })
}

async fn docker_pull(state: &AppState, image: &str) -> Result<()> {
    run_docker(state, ["pull".to_owned(), image.to_owned()])
        .await
        .map(|_| ())
}

async fn docker_login(state: &AppState, credentials: &GithubDockerCredentials) -> Result<()> {
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
        bail!("Docker registry login failed");
    }
    Ok(())
}

async fn docker_logout(state: &AppState) -> Result<()> {
    run_docker(state, ["logout".to_owned(), "ghcr.io".to_owned()])
        .await
        .map(|_| ())
}

async fn remove_existing_container(state: &AppState, container_name: &str) -> Result<()> {
    let output = docker_raw(state, ["inspect".to_owned(), container_name.to_owned()]).await?;
    if !output.status.success() {
        return Ok(());
    }
    run_docker(
        state,
        [
            "rm".to_owned(),
            "--force".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await
    .map(|_| ())
}

async fn docker_inspect_running(state: &AppState, container_name: &str) -> Result<String> {
    let output = run_docker(
        state,
        [
            "inspect".to_owned(),
            "--format={{.State.Running}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    Ok(output.trim().to_owned())
}

async fn docker_port(state: &AppState, container_name: &str, app_port: u16) -> Result<u16> {
    let output = run_docker(
        state,
        [
            "port".to_owned(),
            container_name.to_owned(),
            format!("{app_port}/tcp"),
        ],
    )
    .await?;
    output
        .lines()
        .filter_map(|line| line.rsplit(':').next())
        .find_map(|value| value.trim().parse::<u16>().ok())
        .context("Docker did not publish an app service port")
}

async fn run_docker<const N: usize>(state: &AppState, args: [String; N]) -> Result<String> {
    let output = docker_raw(state, args).await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.lines().next().unwrap_or("unknown Docker error");
        bail!("Docker app service operation failed: {detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn docker_raw<const N: usize>(state: &AppState, args: [String; N]) -> Result<Output> {
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
    fn uses_a_stable_container_name_per_project() {
        let project_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(
            format!("knotree-app-{}", project_id.simple()),
            "knotree-app-11111111222233334444555555555555"
        );
    }
}
