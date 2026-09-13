use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use uuid::Uuid;

use crate::{
    auth,
    cluster::{self, ClusterSpec},
    error::AppError,
    models::{CreateResourceRequest, PostgresResourceResponse},
    projects, security,
    state::AppState,
};

const POSTGRES_RESOURCE_TYPE: &str = "postgres";
const STATUS_READY: &str = "ready";
const STATUS_PROVISIONING: &str = "provisioning";
const STATUS_ERROR: &str = "error";
const PROVIDER_LEGACY_SHARED: &str = "legacy_shared";
const PROVISIONING_ERROR_MESSAGE: &str = "A dedicated PostgreSQL cluster could not be provisioned. Check the cluster provider and try again.";

#[derive(Debug, Clone, sqlx::FromRow)]
struct PostgresResourceRow {
    id: Uuid,
    name: String,
    database_name: String,
    role_name: String,
    host: String,
    port: i32,
    password_ciphertext: String,
    status: String,
    error_message: Option<String>,
    cluster_provider: String,
    cluster_name: Option<String>,
    public_host: Option<String>,
    public_port: Option<i32>,
}

const RESOURCE_COLUMNS: &str = "id, name, database_name, role_name, host, port, password_ciphertext, status, error_message, cluster_provider, cluster_name, public_host, public_port";

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<PostgresResourceResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let resources = sqlx::query_as::<_, PostgresResourceRow>(&format!(
        "SELECT {RESOURCE_COLUMNS} FROM project_postgres_databases WHERE project_id = $1 ORDER BY created_at ASC, id ASC"
    ))
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;

    resources
        .iter()
        .map(|resource| resource_response(resource, &state))
        .collect::<Result<Vec<_>, _>>()
        .map(Json)
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
    Json(input): Json<CreateResourceRequest>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;

    if !state.config.database_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "DATABASE_PROVISIONING_DISABLED",
            message: "PostgreSQL provisioning is not enabled for this environment.",
        });
    }
    if !input
        .resource_type
        .eq_ignore_ascii_case(POSTGRES_RESOURCE_TYPE)
    {
        return Err(AppError::BadRequest {
            code: "UNSUPPORTED_RESOURCE_TYPE",
            message: "Only PostgreSQL resources are available right now.",
        });
    }

    let name = validate_resource_name(input.name.as_deref().unwrap_or("Postgres"))?;
    let (resource, is_new) = {
        let mut transaction = state.db.begin().await?;
        let lock_key = advisory_lock_key(project_id);
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(lock_key)
            .execute(&mut *transaction)
            .await?;

        let resource = match sqlx::query_as::<_, PostgresResourceRow>(&format!(
            "SELECT {RESOURCE_COLUMNS} FROM project_postgres_databases WHERE project_id = $1 FOR UPDATE"
        ))
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            Some(resource) => (resource, false),
            None => {
                let resource_id = Uuid::new_v4();
                let database_name = format!("knotree_db_{}", project_id.simple());
                let role_name = format!("knotree_role_{}", project_id.simple());
                let password = generate_database_password();
                let password_ciphertext = security::encrypt_secret(
                    &password,
                    &state.config.database_credentials_encryption_key,
                )?;
                let resource = sqlx::query_as::<_, PostgresResourceRow>(&format!(
                    "INSERT INTO project_postgres_databases (id, project_id, name, database_name, role_name, host, port, password_ciphertext, status, cluster_provider) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING {RESOURCE_COLUMNS}"
                ))
                .bind(resource_id)
                .bind(project_id)
                .bind(&name)
                .bind(database_name)
                .bind(role_name)
                .bind(&state.config.database_resource_host)
                .bind(i32::from(state.config.database_resource_port))
                .bind(password_ciphertext)
                .bind(STATUS_PROVISIONING)
                .bind(&state.config.database_cluster_provider)
                .fetch_one(&mut *transaction)
                .await?;
                (resource, true)
            }
        };
        transaction.commit().await?;
        resource
    };

    if resource.cluster_provider == PROVIDER_LEGACY_SHARED {
        return Err(AppError::Conflict {
            code: "LEGACY_SHARED_CLUSTER",
            message: "This resource was created on a shared cluster and must be migrated before it can be used with dedicated clusters.",
        });
    }

    let password = security::decrypt_secret(
        &resource.password_ciphertext,
        &state.config.database_credentials_encryption_key,
    )?;
    let provisioned = cluster::provision_with_provider(
        &state.config,
        &resource.cluster_provider,
        &ClusterSpec {
            project_id,
            database_name: resource.database_name.clone(),
            role_name: resource.role_name.clone(),
            password,
        },
    )
    .await;
    let provisioned = match provisioned {
        Ok(provisioned) => provisioned,
        Err(error) => {
            tracing::error!(
                project_id = %project_id,
                resource_id = %resource.id,
                error = %error,
                "postgres resource provisioning failed"
            );
            sqlx::query(
                "UPDATE project_postgres_databases SET status = $1, error_message = $2, updated_at = now() WHERE id = $3",
            )
            .bind(STATUS_ERROR)
            .bind(PROVISIONING_ERROR_MESSAGE)
            .bind(resource.id)
            .execute(&state.db)
            .await?;
            return Err(AppError::ServiceUnavailable {
                code: "DATABASE_PROVISIONING_FAILED",
                message: PROVISIONING_ERROR_MESSAGE,
            });
        }
    };

    let internal_host = provisioned.internal_host.clone();
    let internal_port = i32::from(provisioned.internal_port);
    let response_host = provisioned
        .public_host
        .clone()
        .unwrap_or_else(|| internal_host.clone());
    let response_port = i32::from(provisioned.public_port.unwrap_or(provisioned.internal_port));
    drop(state.remove_database_pool(resource.id));
    let resource = sqlx::query_as::<_, PostgresResourceRow>(&format!(
        "UPDATE project_postgres_databases SET status = $1, error_message = NULL, host = $2, port = $3, cluster_provider = $4, cluster_name = $5, cluster_namespace = $6, cluster_volume = $7, cluster_host = $8, cluster_port = $9, public_host = $10, public_port = $11, updated_at = now() WHERE id = $12 RETURNING {RESOURCE_COLUMNS}"
    ))
    .bind(STATUS_READY)
    .bind(response_host)
    .bind(response_port)
    .bind(provisioned.provider)
    .bind(provisioned.name)
    .bind(provisioned.namespace)
    .bind(provisioned.volume)
    .bind(internal_host)
    .bind(internal_port)
    .bind(provisioned.public_host)
    .bind(provisioned.public_port.map(i32::from))
    .bind(resource.id)
    .fetch_one(&state.db)
    .await?;

    let response = resource_response(&resource, &state)?;

    let status = if is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(response)).into_response())
}

fn validate_resource_name(value: &str) -> Result<String, AppError> {
    let name = value.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "name".to_owned(),
            "Enter a database name between 1 and 80 characters.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(name)
}

fn resource_response(
    resource: &PostgresResourceRow,
    state: &AppState,
) -> Result<PostgresResourceResponse, AppError> {
    let port_value = resource.public_port.unwrap_or(resource.port);
    let host = cluster::connection_host(
        &resource.cluster_provider,
        resource.public_host.as_deref().unwrap_or(&resource.host),
    );
    let port = u16::try_from(port_value)
        .map_err(|_| AppError::internal("invalid postgres resource port"))?;
    let connection_string = if resource.status == STATUS_READY {
        let password = security::decrypt_secret(
            &resource.password_ciphertext,
            &state.config.database_credentials_encryption_key,
        )?;
        Some(connection_string(
            &host,
            port,
            &resource.database_name,
            &resource.role_name,
            &password,
        ))
    } else {
        None
    };
    Ok(PostgresResourceResponse {
        id: resource.id,
        name: resource.name.clone(),
        resource_type: POSTGRES_RESOURCE_TYPE.to_owned(),
        status: resource.status.clone(),
        database_name: resource.database_name.clone(),
        username: resource.role_name.clone(),
        host: host.to_owned(),
        port,
        connection_string,
        cluster_provider: resource.cluster_provider.clone(),
        cluster_name: resource.cluster_name.clone(),
        error_message: resource.error_message.clone(),
    })
}

fn generate_database_password() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn connection_string(
    host: &str,
    port: u16,
    database_name: &str,
    role_name: &str,
    password: &str,
) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    format!("postgres://{role_name}:{password}@{host}:{port}/{database_name}")
}

fn advisory_lock_key(project_id: Uuid) -> i64 {
    let bytes = project_id.into_bytes();
    i64::from_be_bytes(bytes[..8].try_into().expect("uuid has eight leading bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_safe_database_identifiers_and_connection_uri() {
        let project_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let database_name = format!("knotree_db_{}", project_id.simple());
        let role_name = format!("knotree_role_{}", project_id.simple());
        assert_eq!(database_name.len(), 43);
        assert!(database_name.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        }));
        assert_eq!(
            connection_string("localhost", 5432, &database_name, &role_name, "secret"),
            format!("postgres://{role_name}:secret@localhost:5432/{database_name}")
        );
    }

    #[test]
    fn validates_display_name() {
        assert!(validate_resource_name("  Analytics DB ").is_ok());
        assert!(validate_resource_name("\n").is_err());
    }
}
