use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;
use sqlx::FromRow;
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{auth, cluster_kubernetes, error::AppError, projects, security, state::AppState};

pub const REGISTRY_HOST: &str = "registry.knotree.com";
const REGISTRY_URL: &str = "https://registry.knotree.com";
const REGISTRY_TOKEN_SERVICE: &str = "knotree-registry";
const WEBHOOK_CLOCK_SKEW_SECONDS: i64 = 300;
const MAX_REGISTRY_DELIVERY_BYTES: usize = 64 * 1024;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub(crate) struct RegistryDockerCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryConnectionResponse {
    id: Uuid,
    registry_host: String,
    username: String,
    repository: String,
    verified_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RegistryConnectionListResponse {
    connections: Vec<RegistryConnectionResponse>,
    auto_deploy_ready: bool,
    consent_ready: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRegistryConnectionRequest {
    pub username: String,
    pub token: String,
    pub repository: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRegistryConnectionRequest {
    pub token: String,
}

#[derive(Debug, FromRow)]
struct RegistryConnectionRow {
    id: Uuid,
    registry_username: String,
    repository: String,
    verified_at: OffsetDateTime,
}

#[derive(Deserialize)]
struct TokenResponse {
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
}

#[derive(Deserialize)]
struct RegistryEvent {
    schema_version: u32,
    kind: String,
    repository: Option<String>,
    tag: Option<String>,
    digest: Option<String>,
    metadata: RegistryEventMetadata,
}

#[derive(Deserialize)]
struct RegistryEventMetadata {
    registry: Option<String>,
    tagged_image: Option<String>,
    is_tag: Option<bool>,
    #[serde(default)]
    owner_issuer: Option<String>,
    #[serde(default)]
    owner_subject: Option<String>,
}

/// An account-derived connection auto-deploys only when Registry reports the
/// pushed namespace belongs to the same central identity that connected it.
/// Legacy per-repository connections keep their verified-at-consent scope.
fn owner_matches(
    account_identity: Option<(&str, &str)>,
    metadata: &RegistryEventMetadata,
) -> bool {
    match account_identity {
        None => true,
        Some((issuer, subject)) => {
            metadata.owner_issuer.as_deref() == Some(issuer)
                && metadata.owner_subject.as_deref() == Some(subject)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegistryImage {
    pub repository: String,
    pub tag: String,
}

pub async fn list_connections(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug)): Path<(String, String)>,
) -> Result<Json<impl Serialize>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let rows = sqlx::query_as::<_, RegistryConnectionRow>(
        "SELECT id, registry_username, repository, verified_at
         FROM knotree_registry_connections
         WHERE project_id = $1 AND revoked_at IS NULL
           AND (credential_expires_at IS NULL OR credential_expires_at > now())
         ORDER BY created_at DESC, id ASC",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let connections = rows
        .into_iter()
        .filter_map(|row| {
            Some(RegistryConnectionResponse {
                id: row.id,
                registry_host: REGISTRY_HOST.to_owned(),
                username: row.registry_username,
                repository: row.repository,
                verified_at: row
                    .verified_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .ok()?,
            })
        })
        .collect();
    Ok(Json(RegistryConnectionListResponse {
        connections,
        auto_deploy_ready: state.config.knotree_registry_webhook_secret.is_some(),
        consent_ready: state.config.sso.is_some(),
    }))
}

pub async fn create_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug)): Path<(String, String)>,
    Json(input): Json<CreateRegistryConnectionRequest>,
) -> Result<Json<impl Serialize>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let username = validate_registry_username(&input.username)?;
    let token = input.token.trim();
    let repository = validate_registry_repository(&input.repository)?;
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err(validation_error(
            "token",
            "Enter a valid pull-only Knotree Registry token.",
        ));
    }

    if !verify_pull_access(&username, token, &repository).await {
        return Err(AppError::Conflict {
            code: "KNOTREE_REGISTRY_AUTHENTICATION_FAILED",
            message: "Knotree Registry could not verify pull access to this repository.",
        });
    }
    let encrypted =
        security::encrypt_secret(token, &state.config.database_credentials_encryption_key)?;
    let id = Uuid::new_v4();
    let row = sqlx::query_as::<_, RegistryConnectionRow>(
        "INSERT INTO knotree_registry_connections
            (id, project_id, user_id, registry_username, repository, credential_ciphertext)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, registry_username, repository, verified_at",
    )
    .bind(id)
    .bind(project_id)
    .bind(user.id)
    .bind(&username)
    .bind(&repository)
    .bind(encrypted)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(RegistryConnectionResponse {
        id: row.id,
        registry_host: REGISTRY_HOST.to_owned(),
        username: row.registry_username,
        repository: row.repository,
        verified_at: row
            .verified_at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(AppError::internal)?,
    }))
}

pub async fn update_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, connection_id)): Path<(String, String, Uuid)>,
    Json(input): Json<UpdateRegistryConnectionRequest>,
) -> Result<Json<impl Serialize>, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let token = input.token.trim();
    if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err(validation_error(
            "token",
            "Enter a valid pull-only Knotree Registry token.",
        ));
    }
    let connection = sqlx::query_as::<_, RegistryConnectionRow>(
        "SELECT id, registry_username, repository, verified_at
         FROM knotree_registry_connections
         WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL
           AND account_id IS NULL",
    )
    .bind(connection_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "KNOTREE_REGISTRY_CONNECTION_NOT_FOUND",
        message: "The Knotree Registry connection could not be found.",
    })?;
    if !verify_pull_access(&connection.registry_username, token, &connection.repository).await {
        return Err(AppError::Conflict {
            code: "KNOTREE_REGISTRY_AUTHENTICATION_FAILED",
            message: "Knotree Registry could not verify pull access to this repository.",
        });
    }

    let service_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM project_app_services WHERE registry_connection_id = $1",
    )
    .bind(connection_id)
    .fetch_all(&state.db)
    .await?;
    if state.config.uses_kubernetes_workloads() {
        for service_id in &service_ids {
            cluster_kubernetes::update_app_image_pull_secret(
                &state.config,
                *service_id,
                &connection.registry_username,
                token,
            )
            .await
            .map_err(|_| AppError::ServiceUnavailable {
                code: "KNOTREE_REGISTRY_SECRET_UPDATE_FAILED",
                message: "The Registry token was verified, but Kubernetes credentials could not be updated. Retry the rotation.",
            })?;
        }
    }

    let encrypted =
        security::encrypt_secret(token, &state.config.database_credentials_encryption_key)?;
    let row = sqlx::query_as::<_, RegistryConnectionRow>(
        "UPDATE knotree_registry_connections
         SET credential_ciphertext = $1, verified_at = now(), updated_at = now(),
             delegated_credential_id = NULL, credential_expires_at = NULL
         WHERE id = $2 AND project_id = $3 AND revoked_at IS NULL
         RETURNING id, registry_username, repository, verified_at",
    )
    .bind(encrypted)
    .bind(connection_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "KNOTREE_REGISTRY_CONNECTION_NOT_FOUND",
        message: "The Knotree Registry connection could not be found.",
    })?;
    Ok(Json(RegistryConnectionResponse {
        id: row.id,
        registry_host: REGISTRY_HOST.to_owned(),
        username: row.registry_username,
        repository: row.repository,
        verified_at: row
            .verified_at
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(AppError::internal)?,
    }))
}

pub async fn revoke_connection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, connection_id)): Path<(String, String, Uuid)>,
) -> Result<StatusCode, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let mut transaction = state.db.begin().await?;
    let exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(
            SELECT 1 FROM knotree_registry_connections
            WHERE id = $1 AND project_id = $2 AND revoked_at IS NULL
         )",
    )
    .bind(connection_id)
    .bind(project_id)
    .fetch_one(&mut *transaction)
    .await?;
    if !exists {
        return Err(AppError::NotFound {
            code: "KNOTREE_REGISTRY_CONNECTION_NOT_FOUND",
            message: "The Knotree Registry connection could not be found.",
        });
    }
    let service_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM project_app_services
         WHERE registry_connection_id = $1 AND project_id = $2",
    )
    .bind(connection_id)
    .bind(project_id)
    .fetch_all(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE knotree_registry_connections
         SET revoked_at = now(), updated_at = now()
         WHERE id = $1 AND project_id = $2",
    )
    .bind(connection_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE project_app_services
         SET auto_deploy_enabled = FALSE,
             registry_connection_id = NULL,
             auto_deploy_error = 'Knotree Registry connection was disconnected.',
             updated_at = now()
         WHERE registry_connection_id = $1 AND project_id = $2",
    )
    .bind(connection_id)
    .bind(project_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET status = 'failed', locked_until = NULL,
             last_error = 'Knotree Registry connection was disconnected.', updated_at = now()
         WHERE app_service_id = ANY($1) AND status = 'pending'",
    )
    .bind(&service_ids)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    if state.config.uses_kubernetes_workloads() {
        for service_id in service_ids {
            if let Err(error) =
                cluster_kubernetes::delete_app_image_pull_secret(&state.config, service_id).await
            {
                tracing::warn!(
                    registry_connection_id = %connection_id,
                    app_service_id = %service_id,
                    error = %error,
                    "could not remove Knotree Registry Kubernetes pull Secret"
                );
            }
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn load_credentials(
    state: &AppState,
    project_id: Uuid,
    connection_id: Uuid,
) -> Result<Option<RegistryDockerCredentials>, AppError> {
    // Connections created from an account always use the account's current
    // credential, so reconnecting the account renews every derived project.
    let row = sqlx::query_as::<_, (String, String)>(
        "SELECT connection.registry_username,
                COALESCE(account.credential_ciphertext, connection.credential_ciphertext)
         FROM knotree_registry_connections AS connection
         LEFT JOIN knotree_registry_accounts AS account ON account.id = connection.account_id
         WHERE connection.id = $1 AND connection.project_id = $2 AND connection.revoked_at IS NULL
           AND (connection.credential_expires_at IS NULL OR connection.credential_expires_at > now())
           AND (connection.account_id IS NULL
                OR (account.revoked_at IS NULL AND account.credential_expires_at > now()))",
    )
    .bind(connection_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?;
    row.map(|(username, ciphertext)| {
        Ok(RegistryDockerCredentials {
            username,
            password: security::decrypt_secret(
                &ciphertext,
                &state.config.database_credentials_encryption_key,
            )?,
        })
    })
    .transpose()
}

pub(crate) fn parse_registry_image(image: &str) -> Option<RegistryImage> {
    let image = image.trim();
    let host = image.get(..REGISTRY_HOST.len())?;
    if !host.eq_ignore_ascii_case(REGISTRY_HOST)
        || image.get(REGISTRY_HOST.len()..REGISTRY_HOST.len() + 1)? != "/"
    {
        return None;
    }
    let remainder = image.get(REGISTRY_HOST.len() + 1..)?;
    let (repository, tag) = remainder.rsplit_once(':')?;
    if repository.contains('@') || !is_valid_repository(repository) || !is_valid_tag(tag) {
        return None;
    }
    Some(RegistryImage {
        repository: repository.to_owned(),
        tag: tag.to_owned(),
    })
}

pub(crate) fn normalize_registry_image(image: &str) -> Option<String> {
    let parsed = parse_registry_image(image)?;
    Some(format!(
        "{REGISTRY_HOST}/{}:{}",
        parsed.repository, parsed.tag
    ))
}

pub(crate) fn immutable_image(repository: &str, digest: &str) -> Option<String> {
    (is_valid_repository(repository) && is_valid_digest(digest))
        .then(|| format!("{REGISTRY_HOST}/{repository}@{digest}"))
}

pub(crate) fn immutable_reference_for(image: &str, digest: &str) -> Option<String> {
    let parsed = parse_registry_image(image)?;
    immutable_image(&parsed.repository, digest)
}

pub(crate) fn is_valid_digest(digest: &str) -> bool {
    digest.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    })
}

pub(crate) fn digest_from_image_reference(image: &str) -> Option<String> {
    let (registry_and_repo, digest) = image.rsplit_once('@')?;
    (registry_and_repo.starts_with(&format!("{REGISTRY_HOST}/")) && is_valid_digest(digest))
        .then(|| digest.to_owned())
}

pub(crate) fn validate_registry_repository(value: &str) -> Result<String, AppError> {
    let repository = value.trim();
    if !is_valid_repository(repository) {
        return Err(validation_error(
            "repository",
            "Use a Knotree Registry repository such as team/api.",
        ));
    }
    Ok(repository.to_owned())
}

pub(crate) fn validate_registry_username(value: &str) -> Result<String, AppError> {
    let username = value.trim();
    if username.is_empty()
        || username.len() > 128
        || username.contains(':')
        || username.chars().any(char::is_control)
    {
        return Err(validation_error(
            "username",
            "Enter a valid Knotree Registry username.",
        ));
    }
    Ok(username.to_owned())
}

fn is_valid_repository(repository: &str) -> bool {
    !repository.is_empty()
        && repository.len() <= 255
        && repository.split('/').all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| {
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
                })
                && component
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
                && component
                    .bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && (tag.as_bytes()[0].is_ascii_alphanumeric() || tag.starts_with('_'))
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
}

pub(crate) async fn verify_pull_access(username: &str, password: &str, repository: &str) -> bool {
    request_registry_bearer(username, password, repository)
        .await
        .is_some()
}

pub(crate) async fn resolve_tag_digest(
    username: &str,
    password: &str,
    repository: &str,
    tag: &str,
) -> Option<String> {
    if !is_valid_repository(repository) || !is_valid_tag(tag) {
        return None;
    }
    let bearer = request_registry_bearer(username, password, repository).await?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let url = format!("{REGISTRY_URL}/v2/{repository}/manifests/{tag}");
    let response = client
        .get(url)
        .bearer_auth(bearer)
        .header(
            reqwest::header::ACCEPT,
            "application/vnd.oci.image.index.v1+json, application/vnd.oci.image.manifest.v1+json, application/vnd.docker.distribution.manifest.list.v2+json, application/vnd.docker.distribution.manifest.v2+json",
        )
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let digest = response
        .headers()
        .get("docker-content-digest")?
        .to_str()
        .ok()?
        .to_owned();
    is_valid_digest(&digest).then_some(digest)
}

async fn request_registry_bearer(
    username: &str,
    password: &str,
    repository: &str,
) -> Option<String> {
    if !is_valid_repository(repository) {
        return None;
    }
    let Ok(client) = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10))
        .build()
    else {
        return None;
    };
    let token_url = format!(
        "{REGISTRY_URL}/auth/token?service={REGISTRY_TOKEN_SERVICE}&scope=repository:{repository}:pull"
    );
    let response = client
        .get(token_url)
        .basic_auth(username, Some(password))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    response
        .json::<TokenResponse>()
        .await
        .ok()
        .and_then(|response| response.token.or(response.access_token))
        .filter(|token| !token.is_empty())
}

fn validation_error(field: &str, message: &str) -> AppError {
    let mut fields = std::collections::BTreeMap::new();
    fields.insert(field.to_owned(), message.to_owned());
    AppError::validation(fields)
}

pub async fn webhook(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> StatusCode {
    if body.len() > MAX_REGISTRY_DELIVERY_BYTES {
        return StatusCode::PAYLOAD_TOO_LARGE;
    }
    let Some(secret) = state.config.knotree_registry_webhook_secret.as_deref() else {
        return StatusCode::SERVICE_UNAVAILABLE;
    };
    let Some(event_header) = header_value(&headers, "x-knotree-event") else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(delivery_header) = header_value(&headers, "x-knotree-delivery") else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(timestamp_header) = header_value(&headers, "x-knotree-timestamp") else {
        return StatusCode::BAD_REQUEST;
    };
    let Some(signature_header) = header_value(&headers, "x-knotree-signature") else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(delivery_id) = Uuid::parse_str(delivery_header) else {
        return StatusCode::BAD_REQUEST;
    };
    let Ok(timestamp) = timestamp_header.parse::<i64>() else {
        return StatusCode::BAD_REQUEST;
    };
    if OffsetDateTime::now_utc()
        .unix_timestamp()
        .abs_diff(timestamp)
        > WEBHOOK_CLOCK_SKEW_SECONDS as u64
    {
        return StatusCode::UNAUTHORIZED;
    }
    if !verify_webhook_signature(
        secret,
        timestamp_header,
        delivery_header,
        &body,
        signature_header,
    ) {
        return StatusCode::UNAUTHORIZED;
    }
    let Ok(event) = serde_json::from_slice::<RegistryEvent>(&body) else {
        return StatusCode::BAD_REQUEST;
    };
    if event.schema_version != 1 || event.kind != event_header {
        return StatusCode::BAD_REQUEST;
    }
    if event.kind != "tag_updated" {
        return StatusCode::NO_CONTENT;
    }

    match persist_registry_event(&state, delivery_id, event).await {
        Ok(()) => StatusCode::ACCEPTED,
        Err(error) => {
            tracing::error!(
                delivery_id = %delivery_id,
                error = %error,
                "could not persist Knotree Registry webhook delivery"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn verify_webhook_signature(
    secret: &str,
    timestamp: &str,
    delivery_id: &str,
    body: &[u8],
    signature_header: &str,
) -> bool {
    let Some(signature) = signature_header.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(signature) = hex::decode(signature) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(delivery_id.as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = mac.finalize().into_bytes();
    if expected.len() != signature.len() {
        return false;
    }
    expected.as_slice().ct_eq(&signature).unwrap_u8() == 1
}

fn header_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

async fn persist_registry_event(
    state: &AppState,
    delivery_id: Uuid,
    event: RegistryEvent,
) -> Result<(), sqlx::Error> {
    let mut transaction = state.db.begin().await?;
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO knotree_registry_webhook_deliveries (delivery_id, event_kind)
         VALUES ($1, 'tag_updated')
         ON CONFLICT (delivery_id) DO NOTHING
         RETURNING delivery_id",
    )
    .bind(delivery_id)
    .fetch_optional(&mut *transaction)
    .await?;
    if inserted.is_none() {
        transaction.commit().await?;
        return Ok(());
    }

    let Some(repository) = event
        .repository
        .as_deref()
        .filter(|value| is_valid_repository(value))
    else {
        transaction.commit().await?;
        return Ok(());
    };
    let Some(tag) = event.tag.as_deref().filter(|value| is_valid_tag(value)) else {
        transaction.commit().await?;
        return Ok(());
    };
    let Some(digest) = event
        .digest
        .as_deref()
        .filter(|value| is_valid_digest(value))
    else {
        transaction.commit().await?;
        return Ok(());
    };
    if event.metadata.registry.as_deref() != Some(REGISTRY_HOST)
        || event.metadata.is_tag != Some(true)
        || event.metadata.tagged_image.as_deref()
            != Some(format!("{REGISTRY_HOST}/{repository}:{tag}").as_str())
    {
        transaction.commit().await?;
        return Ok(());
    }
    let image_ref =
        immutable_image(repository, digest).expect("event repository and digest were validated");
    let services = sqlx::query_as::<_, (Uuid, String, String, Option<String>, Option<String>)>(
        "SELECT service.id, service.image, connection.repository,
                account.issuer, account.subject
         FROM project_app_services AS service
         JOIN knotree_registry_connections AS connection
           ON connection.id = service.registry_connection_id
         LEFT JOIN knotree_registry_accounts AS account ON account.id = connection.account_id
         WHERE service.image_source = 'knotree_registry'
           AND service.auto_deploy_enabled = TRUE
           AND service.status IN ('ready', 'provisioning')
           AND connection.revoked_at IS NULL
           AND (connection.credential_expires_at IS NULL OR connection.credential_expires_at > now())
           AND (connection.account_id IS NULL
                OR (account.revoked_at IS NULL AND account.credential_expires_at > now()))",
    )
    .fetch_all(&mut *transaction)
    .await?;
    for (service_id, image, connected_repository, account_issuer, account_subject) in services {
        let Some(target) = parse_registry_image(&image) else {
            continue;
        };
        if target.repository != repository
            || target.tag != tag
            || connected_repository != repository
        {
            continue;
        }
        let account_identity = match (account_issuer.as_deref(), account_subject.as_deref()) {
            (Some(issuer), Some(subject)) => Some((issuer, subject)),
            _ => None,
        };
        if !owner_matches(account_identity, &event.metadata) {
            tracing::warn!(
                delivery_id = %delivery_id,
                app_service_id = %service_id,
                "ignoring Knotree Registry push whose owner does not match the connected account"
            );
            continue;
        }
        sqlx::query(
            "INSERT INTO knotree_registry_deploy_jobs
                (id, delivery_id, app_service_id, image_digest, immutable_image)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (delivery_id, app_service_id) DO NOTHING",
        )
        .bind(Uuid::new_v4())
        .bind(delivery_id)
        .bind(service_id)
        .bind(digest)
        .bind(&image_ref)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await
}

pub(crate) fn docker_config_json(
    username: &str,
    password: &str,
) -> Result<Value, serde_json::Error> {
    let auth = STANDARD.encode(format!("{username}:{password}"));
    serde_json::from_value(serde_json::json!({
        "auths": {
            REGISTRY_HOST: { "auth": auth }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sign(secret: &str, timestamp: &str, delivery_id: &str, body: &[u8]) -> String {
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(timestamp.as_bytes());
        mac.update(b".");
        mac.update(delivery_id.as_bytes());
        mac.update(b".");
        mac.update(body);
        format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
    }

    #[test]
    fn parses_only_fixed_registry_images_with_explicit_tags() {
        assert_eq!(
            parse_registry_image("registry.knotree.com/team/api:production"),
            Some(RegistryImage {
                repository: "team/api".to_owned(),
                tag: "production".to_owned(),
            })
        );
        assert!(parse_registry_image("registry.knotree.com/team/api").is_none());
        assert!(parse_registry_image("evil.example/team/api:production").is_none());
        assert!(parse_registry_image("registry.knotree.com/team/../api:production").is_none());
    }

    #[test]
    fn validates_digest_and_builds_immutable_image_from_trusted_components() {
        let digest = format!("sha256:{}", "a".repeat(64));
        assert_eq!(
            immutable_image("team/api", &digest).as_deref(),
            Some(format!("{REGISTRY_HOST}/team/api@{digest}").as_str())
        );
        assert!(immutable_image("team/api", "sha256:invalid").is_none());
        assert!(immutable_image("team/../api", &digest).is_none());
    }

    #[test]
    fn verifies_hmac_against_exact_timestamp_delivery_and_raw_body() {
        let body = br#"{"kind":"tag_updated"}"#;
        let signature = sign("secret-value", "123", "delivery-id", body);
        assert!(verify_webhook_signature(
            "secret-value",
            "123",
            "delivery-id",
            body,
            &signature
        ));
        assert!(!verify_webhook_signature(
            "other-secret",
            "123",
            "delivery-id",
            body,
            &signature
        ));
        assert!(!verify_webhook_signature(
            "secret-value",
            "123",
            "delivery-id",
            br#"{"kind":"tag_updated"} "#,
            &signature
        ));
    }

    #[test]
    fn account_connections_deploy_only_for_matching_owner() {
        let metadata = |issuer: Option<&str>, subject: Option<&str>| RegistryEventMetadata {
            registry: Some(REGISTRY_HOST.into()),
            tagged_image: None,
            is_tag: Some(true),
            owner_issuer: issuer.map(Into::into),
            owner_subject: subject.map(Into::into),
        };
        let issuer = "https://accounts.knotree.com";
        let alice = Some((issuer, "alice"));
        assert!(owner_matches(alice, &metadata(Some(issuer), Some("alice"))));
        assert!(!owner_matches(alice, &metadata(Some(issuer), Some("bob"))));
        assert!(!owner_matches(alice, &metadata(Some("https://evil.example"), Some("alice"))));
        assert!(!owner_matches(alice, &metadata(None, None)));
        // Legacy repository-scoped connections are unaffected.
        assert!(owner_matches(None, &metadata(None, None)));
    }

    #[test]
    fn creates_registry_specific_docker_auth_without_returning_plaintext() {
        let config = docker_config_json("service-user", "pull-token").unwrap();
        let auth = config["auths"][REGISTRY_HOST]["auth"].as_str().unwrap();
        assert_eq!(STANDARD.decode(auth).unwrap(), b"service-user:pull-token");
        assert!(!config.to_string().contains("pull-token"));
    }
}
