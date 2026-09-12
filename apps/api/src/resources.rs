use std::str::FromStr;

use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use uuid::Uuid;

use crate::{
    auth,
    error::AppError,
    models::{CreateResourceRequest, PostgresResourceResponse},
    projects, security,
    state::AppState,
};

const POSTGRES_RESOURCE_TYPE: &str = "postgres";
const STATUS_READY: &str = "ready";
const STATUS_PROVISIONING: &str = "provisioning";
const STATUS_ERROR: &str = "error";
const PROVISIONING_ERROR_MESSAGE: &str =
    "PostgreSQL could not be provisioned. Check the API database permissions and try again.";

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
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<PostgresResourceResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let resources = sqlx::query_as::<_, PostgresResourceRow>(
        "SELECT id, name, database_name, role_name, host, port, password_ciphertext, status, error_message FROM project_postgres_databases WHERE project_id = $1 ORDER BY created_at ASC, id ASC",
    )
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
    let mut transaction = state.db.begin().await?;
    let lock_key = advisory_lock_key(project_id);
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(lock_key)
        .execute(&mut *transaction)
        .await?;

    let (resource, is_new) = match sqlx::query_as::<_, PostgresResourceRow>(
        "SELECT id, name, database_name, role_name, host, port, password_ciphertext, status, error_message FROM project_postgres_databases WHERE project_id = $1 FOR UPDATE",
    )
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
            let password_ciphertext =
                security::encrypt_secret(&password, &state.config.database_credentials_encryption_key)?;
            let resource = sqlx::query_as::<_, PostgresResourceRow>(
                "INSERT INTO project_postgres_databases (id, project_id, name, database_name, role_name, host, port, password_ciphertext, status) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id, name, database_name, role_name, host, port, password_ciphertext, status, error_message",
            )
            .bind(resource_id)
            .bind(project_id)
            .bind(&name)
            .bind(database_name)
            .bind(role_name)
            .bind(&state.config.database_resource_host)
            .bind(i32::from(state.config.database_resource_port))
            .bind(password_ciphertext)
            .bind(STATUS_PROVISIONING)
            .fetch_one(&mut *transaction)
            .await?;
            (resource, true)
        }
    };

    if resource.status == STATUS_READY {
        let response = resource_response(&resource, &state)?;
        transaction.commit().await?;
        return Ok((StatusCode::OK, Json(response)).into_response());
    }

    let password = security::decrypt_secret(
        &resource.password_ciphertext,
        &state.config.database_credentials_encryption_key,
    )?;
    if let Err(error) = provision_postgres_database(
        &state.config.database_url,
        &resource.database_name,
        &resource.role_name,
        &password,
    )
    .await
    {
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
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        return Err(AppError::ServiceUnavailable {
            code: "DATABASE_PROVISIONING_FAILED",
            message: PROVISIONING_ERROR_MESSAGE,
        });
    }

    let resource = sqlx::query_as::<_, PostgresResourceRow>(
        "UPDATE project_postgres_databases SET status = $1, error_message = NULL, updated_at = now() WHERE id = $2 RETURNING id, name, database_name, role_name, host, port, password_ciphertext, status, error_message",
    )
    .bind(STATUS_READY)
    .bind(resource.id)
    .fetch_one(&mut *transaction)
    .await?;
    let response = resource_response(&resource, &state)?;
    transaction.commit().await?;

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
    let port = u16::try_from(resource.port)
        .map_err(|_| AppError::internal("invalid postgres resource port"))?;
    let connection_string = if resource.status == STATUS_READY {
        let password = security::decrypt_secret(
            &resource.password_ciphertext,
            &state.config.database_credentials_encryption_key,
        )?;
        Some(connection_string(
            &resource.host,
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
        host: resource.host.clone(),
        port,
        connection_string,
        error_message: resource.error_message.clone(),
    })
}

async fn provision_postgres_database(
    database_url: &str,
    database_name: &str,
    role_name: &str,
    password: &str,
) -> Result<()> {
    let base_options = PgConnectOptions::from_str(database_url)
        .context("DATABASE_URL is not a valid PostgreSQL connection string")?;
    let mut connection = PgConnection::connect_with(&base_options)
        .await
        .context("could not connect to the PostgreSQL administration database")?;

    let role_exists =
        sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(role_name)
            .fetch_one(&mut connection)
            .await?;
    let quoted_role = quote_identifier(role_name);
    let quoted_password = quote_literal(password);
    if role_exists {
        sqlx::query(&format!(
            "ALTER ROLE {quoted_role} LOGIN PASSWORD {quoted_password}"
        ))
        .execute(&mut connection)
        .await?;
    } else {
        sqlx::query(&format!(
            "CREATE ROLE {quoted_role} LOGIN PASSWORD {quoted_password}"
        ))
        .execute(&mut connection)
        .await?;
    }

    let existing_owner = sqlx::query_scalar::<_, String>(
        "SELECT pg_get_userbyid(datdba) FROM pg_database WHERE datname = $1",
    )
    .bind(database_name)
    .fetch_optional(&mut connection)
    .await?;
    let quoted_database = quote_identifier(database_name);
    match existing_owner {
        Some(owner) if owner != role_name => {
            bail!("database identifier is already owned by another role")
        }
        Some(_) => {}
        None => {
            sqlx::query(&format!(
                "CREATE DATABASE {quoted_database} OWNER {quoted_role}"
            ))
            .execute(&mut connection)
            .await?;
        }
    }

    sqlx::query(&format!(
        "ALTER DATABASE {quoted_database} ALLOW_CONNECTIONS true"
    ))
    .execute(&mut connection)
    .await?;
    sqlx::query(&format!(
        "REVOKE CONNECT ON DATABASE {quoted_database} FROM PUBLIC"
    ))
    .execute(&mut connection)
    .await?;
    sqlx::query(&format!(
        "GRANT CONNECT ON DATABASE {quoted_database} TO {quoted_role}"
    ))
    .execute(&mut connection)
    .await?;

    let resource_options = base_options
        .username(role_name)
        .password(password)
        .database(database_name);
    let mut resource_connection = PgConnection::connect_with(&resource_options)
        .await
        .context("created PostgreSQL database could not accept a connection")?;
    sqlx::query("SELECT 1")
        .execute(&mut resource_connection)
        .await?;
    resource_connection.close().await?;
    connection.close().await?;
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn quote_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
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
