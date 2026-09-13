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
    cluster::{self, PROVIDER_DOCKER},
    error::AppError,
    github::{self, GithubDockerCredentials},
    models::{AppServiceDatabaseConnectionResponse, AppServiceResponse, CreateAppServiceRequest},
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
}

const APP_SERVICE_COLUMNS: &str = "id, project_id, name, image, image_source, app_port, host, port, container_name, status, error_message, database_resource_id";

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
    let service = sqlx::query_as::<_, AppServiceRow>(&format!(
        "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE project_id = $1"
    ))
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;

    let database = ready_database_resource(&state, project_id).await?;
    service
        .map(|service| {
            app_service_response(
                &service,
                &state.config.app_service_public_host,
                database.as_ref(),
            )
        })
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
    let database = ready_database_resource(&state, project_id).await?;
    let database_resource_id = database.as_ref().map(|database| database.id);

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
            Some(service)
                if service.status == STATUS_READY
                    && service.database_resource_id == database_resource_id =>
            {
                (service, false)
            }
            Some(service) if service.status == STATUS_PROVISIONING => {
                return Err(AppError::Conflict {
                    code: "APP_SERVICE_PROVISIONING",
                    message: "This app service is already being deployed.",
                });
            }
            Some(service) => {
                let service = sqlx::query_as::<_, AppServiceRow>(&format!(
                    "UPDATE project_app_services SET name = $1, image = $2, image_source = $3, app_port = $4, host = NULL, port = NULL, status = $5, error_message = NULL, database_resource_id = $6, updated_at = now() WHERE id = $7 RETURNING {APP_SERVICE_COLUMNS}"
                ))
                .bind(&name)
                .bind(&image)
                .bind(&image_source)
                .bind(i32::from(app_port))
                .bind(STATUS_PROVISIONING)
                .bind(database_resource_id)
                .bind(service.id)
                .fetch_one(&mut *transaction)
                .await?;
                (service, false)
            }
            None => {
                let service = sqlx::query_as::<_, AppServiceRow>(&format!(
                    "INSERT INTO project_app_services (id, project_id, name, image, image_source, app_port, status, database_resource_id) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {APP_SERVICE_COLUMNS}"
                ))
                .bind(Uuid::new_v4())
                .bind(project_id)
                .bind(&name)
                .bind(&image)
                .bind(&image_source)
                .bind(i32::from(app_port))
                .bind(STATUS_PROVISIONING)
                .bind(database_resource_id)
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
                database.as_ref(),
            )?),
        )
            .into_response());
    }

    let github_credentials = if image_source == IMAGE_SOURCE_GITHUB {
        match github::docker_credentials(&state, user.id).await? {
            Some(credentials) => Some(credentials),
            None => {
                sqlx::query(
                    "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = NULL, updated_at = now() WHERE id = $3",
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
        database.as_ref(),
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
                "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = NULL, updated_at = now() WHERE id = $3",
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
        "UPDATE project_app_services SET status = $1, host = $2, port = $3, app_port = $4, container_name = $5, database_resource_id = $6, error_message = NULL, updated_at = now() WHERE id = $7 RETURNING {APP_SERVICE_COLUMNS}"
    ))
    .bind(STATUS_READY)
    .bind(&provisioned.host)
    .bind(i32::from(provisioned.port))
    .bind(i32::from(provisioned.app_port))
    .bind(&provisioned.container_name)
    .bind(database_resource_id)
    .bind(service.id)
    .fetch_one(&state.db)
    .await?;
    let response = app_service_response(
        &service,
        &state.config.app_service_public_host,
        database.as_ref(),
    )?;
    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(response)).into_response())
}

/// Attach a previously deployed app to a newly-ready project database. This
/// covers the reverse creation order (app first, Postgres second) while
/// keeping the database endpoint itself independent from the app API.
pub async fn reconcile_project_database_connection(
    state: &AppState,
    project_id: Uuid,
    user_id: Uuid,
) -> Result<()> {
    let Some(database) = ready_database_resource(state, project_id).await? else {
        return Ok(());
    };

    let Some(service) = ({
        let mut transaction = state.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *transaction)
            .await?;
        let service = sqlx::query_as::<_, AppServiceRow>(&format!(
            "SELECT {APP_SERVICE_COLUMNS} FROM project_app_services WHERE project_id = $1 AND status = $2 AND (database_resource_id IS NULL OR database_resource_id <> $3) FOR UPDATE"
        ))
        .bind(project_id)
        .bind(STATUS_READY)
        .bind(database.id)
        .fetch_optional(&mut *transaction)
        .await?;
        if let Some(service) = service.as_ref() {
            sqlx::query(
                "UPDATE project_app_services SET status = $1, error_message = NULL, updated_at = now() WHERE id = $2",
            )
            .bind(STATUS_PROVISIONING)
            .bind(service.id)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        service
    }) else {
        return Ok(());
    };

    let github_credentials = if service.image_source == IMAGE_SOURCE_GITHUB {
        match github::docker_credentials(state, user_id)
            .await
            .map_err(|_| anyhow::anyhow!("could not read GitHub package credentials"))?
        {
            Some(credentials) => Some(credentials),
            None => {
                // The existing app remains untouched. The next explicit
                // deploy, after GitHub is connected, will include the DB env.
                tracing::warn!(
                    project_id = %project_id,
                    service_id = %service.id,
                    "skipping automatic private app database connection without GitHub credentials"
                );
                sqlx::query(
                    "UPDATE project_app_services SET status = $1, updated_at = now() WHERE id = $2",
                )
                .bind(STATUS_READY)
                .bind(service.id)
                .execute(&state.db)
                .await?;
                return Ok(());
            }
        }
    } else {
        None
    };

    let provisioned = provision_docker(
        state,
        project_id,
        &service.image,
        u16::try_from(service.app_port).context("invalid app service container port")?,
        github_credentials.as_ref(),
        Some(&database),
    )
    .await;
    match provisioned {
        Ok(provisioned) => {
            sqlx::query(
                "UPDATE project_app_services SET status = $1, host = $2, port = $3, app_port = $4, container_name = $5, database_resource_id = $6, error_message = NULL, updated_at = now() WHERE id = $7",
            )
            .bind(STATUS_READY)
            .bind(&provisioned.host)
            .bind(i32::from(provisioned.port))
            .bind(i32::from(provisioned.app_port))
            .bind(&provisioned.container_name)
            .bind(database.id)
            .bind(service.id)
            .execute(&state.db)
            .await?;
            Ok(())
        }
        Err(error) => {
            tracing::error!(
                project_id = %project_id,
                service_id = %service.id,
                error = %error,
                "automatic app service database connection failed"
            );
            sqlx::query(
                "UPDATE project_app_services SET status = $1, error_message = $2, database_resource_id = NULL, updated_at = now() WHERE id = $3",
            )
            .bind(STATUS_ERROR)
            .bind(PROVISIONING_ERROR_MESSAGE)
            .bind(service.id)
            .execute(&state.db)
            .await?;
            Err(error)
        }
    }
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
    database: Option<&DatabaseResourceRow>,
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
        database_connection: service
            .database_resource_id
            .and_then(|resource_id| database.filter(|database| database.id == resource_id))
            .map(|database| database_connection_response(service.project_id, database)),
    })
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

async fn ready_database_resource(
    state: &AppState,
    project_id: Uuid,
) -> std::result::Result<Option<DatabaseResourceRow>, sqlx::Error> {
    sqlx::query_as::<_, DatabaseResourceRow>(&format!(
        "SELECT {DATABASE_RESOURCE_COLUMNS} FROM project_postgres_databases WHERE project_id = $1 AND status = 'ready' AND cluster_provider = $2"
    ))
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
    image: &str,
    app_port: u16,
    github_credentials: Option<&GithubDockerCredentials>,
    database: Option<&DatabaseResourceRow>,
) -> Result<ProvisionedAppService> {
    let database_environment =
        database_environment(&state.config.database_credentials_encryption_key, database)?;
    let network_name = cluster::ensure_project_network(&state.config, project_id).await?;

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

    let exposed = docker_image_exposed_ports(state, image).await?;
    let app_port = resolve_container_port(app_port, &exposed);

    let container_name = format!("knotree-app-{}", project_id.simple());
    remove_existing_container(state, &container_name).await?;
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
    // Keep database credentials inside the container environment. They are
    // never serialized into the API response or Docker labels.
    for variable in database_environment {
        docker_args.push("--env".to_owned());
        docker_args.push(variable);
    }
    docker_args.push(image.to_owned());
    run_docker(state, docker_args).await?;

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
        app_port,
        container_name,
    })
}

async fn docker_image_exposed_ports(state: &AppState, image: &str) -> Result<Vec<u16>> {
    let output = run_docker(
        state,
        [
            "inspect".to_owned(),
            "--format={{range $port, $_ := .Config.ExposedPorts}}{{$port}} {{end}}".to_owned(),
            image.to_owned(),
        ],
    )
    .await?;
    Ok(parse_exposed_ports(&output))
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
    fn maps_publish_port_to_image_expose_when_requested_port_is_absent() {
        assert_eq!(parse_exposed_ports(r#"{"80/tcp":{}}"#), vec![80]);
        assert_eq!(parse_exposed_ports("80/tcp 443/tcp"), vec![80, 443]);
        assert_eq!(parse_exposed_ports(""), Vec::<u16>::new());
        assert_eq!(resolve_container_port(8080, &[80]), 80);
        assert_eq!(resolve_container_port(80, &[80, 443]), 80);
        assert_eq!(resolve_container_port(3000, &[]), 3000);
        assert_eq!(resolve_container_port(9000, &[8080, 8443]), 8080);
    }

    #[test]
    fn uses_a_stable_container_name_per_project() {
        let project_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(
            format!("knotree-app-{}", project_id.simple()),
            "knotree-app-11111111222233334444555555555555"
        );
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
}
