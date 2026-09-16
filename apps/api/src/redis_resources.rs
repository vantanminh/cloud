use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    auth,
    cluster::{self, ClusterSpec, PROVIDER_DOCKER, PROVIDER_KUBERNETES},
    config::Config,
    error::AppError,
    limits::TENANT_RESOURCE_CAPS,
    models::{CreateResourceRequest, RedisResourceResponse},
    projects, security,
    state::AppState,
};

const REDIS_RESOURCE_TYPE: &str = "redis";
const STATUS_READY: &str = "ready";
const STATUS_PROVISIONING: &str = "provisioning";
const STATUS_ERROR: &str = "error";
const PROVISIONING_ERROR_MESSAGE: &str =
    "A Redis instance could not be provisioned. Check the cluster provider and try again.";

#[derive(Debug, Clone, sqlx::FromRow)]
struct RedisResourceRow {
    id: Uuid,
    project_id: Uuid,
    name: String,
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

const RESOURCE_COLUMNS: &str = "id, project_id, name, host, port, password_ciphertext, status, error_message, cluster_provider, cluster_name, public_host, public_port";

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<RedisResourceResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    list_for_user(&state, user.id, &workspace_slug, &project_slug)
        .await
        .map(Json)
}

pub async fn list_for_user(
    state: &AppState,
    user_id: Uuid,
    workspace_slug: &str,
    project_slug: &str,
) -> Result<Vec<RedisResourceResponse>, AppError> {
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_slug, project_slug).await?;
    let resources = sqlx::query_as::<_, RedisResourceRow>(&format!(
        "SELECT {RESOURCE_COLUMNS} FROM project_redis_instances WHERE project_id = $1 ORDER BY created_at ASC, id ASC"
    ))
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    resources
        .iter()
        .map(|resource| resource_response(resource, &state.config))
        .collect()
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
    Json(input): Json<CreateResourceRequest>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let resource = create_for_user(
        &state,
        user.id,
        &workspace_slug,
        &project_slug,
        input.name.as_deref(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(resource)).into_response())
}

pub async fn mcp_create(
    state: &AppState,
    user_id: Uuid,
    workspace_slug: &str,
    project_slug: &str,
    name: Option<&str>,
) -> Result<Value, AppError> {
    let resource = create_for_user(state, user_id, workspace_slug, project_slug, name).await?;
    serde_json::to_value(resource).map_err(AppError::internal)
}

pub async fn create_for_user(
    state: &AppState,
    user_id: Uuid,
    workspace_slug: &str,
    project_slug: &str,
    name: Option<&str>,
) -> Result<RedisResourceResponse, AppError> {
    if !state.config.database_provisioning_enabled {
        return Err(AppError::ServiceUnavailable {
            code: "REDIS_PROVISIONING_DISABLED",
            message: "Redis provisioning is not enabled for this environment.",
        });
    }
    let project_id =
        projects::accessible_project_id(state, user_id, workspace_slug, project_slug).await?;
    let name = validate_resource_name(name.unwrap_or("Redis"))?;
    let (resource, is_new) = {
        let mut transaction = state.db.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(advisory_lock_key(project_id))
            .execute(&mut *transaction)
            .await?;
        let created = match sqlx::query_as::<_, RedisResourceRow>(&format!(
            "SELECT {RESOURCE_COLUMNS} FROM project_redis_instances WHERE project_id = $1 FOR UPDATE"
        ))
        .bind(project_id)
        .fetch_optional(&mut *transaction)
        .await?
        {
            Some(resource) => (resource, false),
            None => {
                let password = generate_password();
                let password_ciphertext = security::encrypt_secret(
                    &password,
                    &state.config.database_credentials_encryption_key,
                )?;
                let resource = sqlx::query_as::<_, RedisResourceRow>(&format!(
                    "INSERT INTO project_redis_instances (id, project_id, name, password_ciphertext, host, port, status, cluster_provider) VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {RESOURCE_COLUMNS}"
                ))
                .bind(Uuid::new_v4())
                .bind(project_id)
                .bind(&name)
                .bind(password_ciphertext)
                .bind(&state.config.database_resource_host)
                .bind(6379_i32)
                .bind(STATUS_PROVISIONING)
                .bind(&state.config.database_cluster_provider)
                .fetch_one(&mut *transaction)
                .await?;
                (resource, true)
            }
        };
        transaction.commit().await?;
        created
    };

    if !is_new && resource.status == STATUS_READY {
        return resource_response(&resource, &state.config);
    }

    let password = security::decrypt_secret(
        &resource.password_ciphertext,
        &state.config.database_credentials_encryption_key,
    )?;
    let provisioned = cluster::provision_redis(
        &state.config,
        &ClusterSpec {
            project_id,
            database_name: "redis".to_owned(),
            role_name: "default".to_owned(),
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
                "redis resource provisioning failed"
            );
            sqlx::query(
                "UPDATE project_redis_instances SET status = $1, error_message = $2, updated_at = now() WHERE id = $3",
            )
            .bind(STATUS_ERROR)
            .bind(PROVISIONING_ERROR_MESSAGE)
            .bind(resource.id)
            .execute(&state.db)
            .await?;
            return Err(AppError::ServiceUnavailable {
                code: "REDIS_PROVISIONING_FAILED",
                message: PROVISIONING_ERROR_MESSAGE,
            });
        }
    };

    let response_host = provisioned
        .public_host
        .clone()
        .unwrap_or_else(|| provisioned.internal_host.clone());
    let response_port = i32::from(provisioned.public_port.unwrap_or(provisioned.internal_port));
    let resource = sqlx::query_as::<_, RedisResourceRow>(&format!(
        "UPDATE project_redis_instances SET status = $1, error_message = NULL, host = $2, port = $3, cluster_provider = $4, cluster_name = $5, cluster_namespace = $6, cluster_volume = $7, public_host = $8, public_port = $9, updated_at = now() WHERE id = $10 RETURNING {RESOURCE_COLUMNS}"
    ))
    .bind(STATUS_READY)
    .bind(response_host)
    .bind(response_port)
    .bind(provisioned.provider)
    .bind(provisioned.name)
    .bind(provisioned.namespace)
    .bind(provisioned.volume)
    .bind(provisioned.public_host)
    .bind(provisioned.public_port.map(i32::from))
    .bind(resource.id)
    .fetch_one(&state.db)
    .await?;
    resource_response(&resource, &state.config)
}

fn validate_resource_name(value: &str) -> Result<String, AppError> {
    let name = value.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "name".to_owned(),
            "Enter a Redis name between 1 and 80 characters.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(name)
}

fn resource_response(
    resource: &RedisResourceRow,
    config: &Config,
) -> Result<RedisResourceResponse, AppError> {
    let port_value = resource.public_port.unwrap_or(resource.port);
    let network_alias = advertised_redis_network_alias(
        &resource.cluster_provider,
        resource.cluster_name.as_deref(),
        resource.project_id,
    );
    let host = advertised_redis_host(
        &resource.cluster_provider,
        resource.public_host.as_deref().unwrap_or(&resource.host),
        &network_alias,
        &config.database_cluster_namespace,
    );
    let port = u16::try_from(port_value).map_err(|_| AppError::internal("invalid redis port"))?;
    let connection_string = if resource.status == STATUS_READY {
        let password = security::decrypt_secret(
            &resource.password_ciphertext,
            &config.database_credentials_encryption_key,
        )?;
        Some(format!("redis://:{password}@{host}:{port}"))
    } else {
        None
    };
    Ok(RedisResourceResponse {
        id: resource.id,
        name: resource.name.clone(),
        resource_type: REDIS_RESOURCE_TYPE.to_owned(),
        status: resource.status.clone(),
        host,
        port,
        connection_string,
        cluster_provider: resource.cluster_provider.clone(),
        cluster_name: resource.cluster_name.clone(),
        error_message: resource.error_message.clone(),
        network_alias,
        cpu_limit: TENANT_RESOURCE_CAPS.cpu.to_owned(),
        memory_limit: TENANT_RESOURCE_CAPS.memory_kubernetes.to_owned(),
        storage_limit: TENANT_RESOURCE_CAPS.storage_kubernetes.to_owned(),
    })
}

pub fn advertised_redis_network_alias(
    provider: &str,
    cluster_name: Option<&str>,
    project_id: Uuid,
) -> String {
    if provider == PROVIDER_KUBERNETES {
        cluster_name
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| crate::cluster_kubernetes::redis_resource_name(project_id))
    } else {
        cluster::PROJECT_NETWORK_REDIS_ALIAS.to_owned()
    }
}

fn advertised_redis_host(
    provider: &str,
    stored_host: &str,
    network_alias: &str,
    namespace: &str,
) -> String {
    if provider == PROVIDER_KUBERNETES {
        if stored_host.contains(".svc.cluster.local") {
            stored_host.to_owned()
        } else {
            format!("{network_alias}.{namespace}.svc.cluster.local")
        }
    } else {
        cluster::connection_host(provider, stored_host)
    }
}

fn generate_password() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn advisory_lock_key(project_id: Uuid) -> i64 {
    let mut bytes = project_id.into_bytes();
    bytes[0] ^= 0x51;
    i64::from_be_bytes(bytes[..8].try_into().expect("uuid has eight leading bytes"))
}

pub fn redis_joined_private_network(provider: &str, network_name: Option<&str>) -> bool {
    match provider {
        PROVIDER_DOCKER => network_name.is_some(),
        PROVIDER_KUBERNETES => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redis_create_joins_the_project_private_network() {
        assert!(redis_joined_private_network(
            PROVIDER_DOCKER,
            Some("knotree-net-abc")
        ));
        assert!(!redis_joined_private_network(PROVIDER_DOCKER, None));
        assert!(redis_joined_private_network(PROVIDER_KUBERNETES, None));
        assert_eq!(cluster::PROJECT_NETWORK_REDIS_ALIAS, "redis");
        assert_eq!(TENANT_RESOURCE_CAPS.memory_kubernetes, "1Gi");
        let project_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let k8s_alias = advertised_redis_network_alias(
            PROVIDER_KUBERNETES,
            Some("knotree-redis-11111111222233334444555555555555"),
            project_id,
        );
        assert_eq!(
            k8s_alias,
            "knotree-redis-11111111222233334444555555555555"
        );
        assert_ne!(k8s_alias, cluster::PROJECT_NETWORK_REDIS_ALIAS);
        assert_eq!(
            advertised_redis_network_alias(PROVIDER_DOCKER, None, project_id),
            "redis"
        );
        assert_eq!(
            advertised_redis_host(
                PROVIDER_KUBERNETES,
                "knotree-redis-11111111222233334444555555555555.knotree-cloud.svc.cluster.local",
                &k8s_alias,
                "knotree-cloud",
            ),
            "knotree-redis-11111111222233334444555555555555.knotree-cloud.svc.cluster.local"
        );
    }

    #[test]
    fn validates_display_name() {
        assert_eq!(validate_resource_name(" Cache ").unwrap(), "Cache");
        assert!(validate_resource_name("").is_err());
        assert!(validate_resource_name(&"n".repeat(81)).is_err());
    }
}
