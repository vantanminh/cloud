//! Account-level Knotree Registry connection ("connect once", like installing
//! a GitHub App). One SSO-bound consent gives Cloud a pull-only credential for
//! the user's whole Registry namespace. The picker lists only that namespace,
//! and project connections created from it always use the account credential.

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use crate::{
    auth, error::AppError, knotree_registry, projects, registry_consent, security,
    state::AppState,
};

const REGISTRY: &str = "https://registry.knotree.com";
const MAX_LIST_BYTES: usize = 1024 * 1024;

fn invalid() -> AppError {
    AppError::BadRequest {
        code: "REGISTRY_CONSENT_FAILED",
        message: "Registry authorization expired or could not be verified. Start again.",
    }
}

fn not_connected() -> AppError {
    AppError::Conflict {
        code: "KNOTREE_REGISTRY_ACCOUNT_REQUIRED",
        message: "Connect your Knotree Registry account first.",
    }
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

/// Only same-site relative paths, so the callback can never redirect off Cloud.
fn safe_return_to(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    (value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains('\\')
        && value.len() <= 512
        && !value.chars().any(char::is_control))
    .then(|| value.to_owned())
}

async fn sso_subject(state: &AppState, user_id: Uuid, issuer: &str) -> Result<Option<String>, AppError> {
    Ok(
        sqlx::query_scalar("SELECT subject FROM sso_identities WHERE user_id=$1 AND issuer=$2")
            .bind(user_id)
            .bind(issuer)
            .fetch_optional(&state.db)
            .await?,
    )
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StartInput {
    #[serde(default)]
    return_to: Option<String>,
}

#[derive(Deserialize)]
struct Started {
    request_id: Uuid,
    authorization_url: String,
}

/// POST /integrations/knotree-registry/authorize
pub async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: Option<Json<StartInput>>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let config = state.config.sso.as_ref().ok_or(AppError::ServiceUnavailable {
        code: "SSO_NOT_CONFIGURED",
        message: "Sign in with Knotree Accounts to connect Registry.",
    })?;
    let subject = sso_subject(&state, user.id, &config.issuer)
        .await?
        .ok_or(AppError::Forbidden {
            code: "ACCOUNTS_SIGN_IN_REQUIRED",
            message: "This Cloud account must be linked to Knotree Accounts before connecting Registry.",
        })?;
    let return_to = safe_return_to(input.as_ref().and_then(|v| v.return_to.as_deref()));
    let state_token = security::random_token();
    let verifier = security::random_token();
    let challenge = URL_SAFE_NO_PAD.encode(security::token_hash(&verifier));
    let response = registry_consent::client()?
        .post(format!("{REGISTRY}/api/v1/cloud-grants/requests"))
        .json(&serde_json::json!({
            "client_id": registry_consent::CLIENT,
            "redirect_uri": registry_consent::CALLBACK,
            "state": state_token,
            "namespace": true,
            "code_challenge": challenge,
            "code_challenge_method": "S256",
            "expected_issuer": config.issuer,
            "expected_subject": subject,
        }))
        .send()
        .await
        .map_err(|_| invalid())?;
    let started: Started = registry_consent::bounded_json(response).await?;
    if started.authorization_url != format!("{REGISTRY}/cloud/authorize/{}", started.request_id) {
        return Err(invalid());
    }
    sqlx::query("DELETE FROM registry_account_consent_attempts WHERE expires_at <= now() OR user_id=$1")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "INSERT INTO registry_account_consent_attempts
            (state_hash, session_hash, user_id, issuer, subject, verifier_ciphertext, return_to, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, now() + interval '10 minutes')",
    )
    .bind(security::token_hash(&state_token))
    .bind(registry_consent::session_hash(&state, &headers)?)
    .bind(user.id)
    .bind(&config.issuer)
    .bind(subject)
    .bind(security::encrypt_secret(
        &verifier,
        &state.config.database_credentials_encryption_key,
    )?)
    .bind(return_to)
    .execute(&state.db)
    .await?;
    Ok(no_store(
        Json(serde_json::json!({"authorizationUrl": started.authorization_url})).into_response(),
    ))
}

#[derive(FromRow)]
struct Attempt {
    issuer: String,
    subject: String,
    verifier_ciphertext: String,
    return_to: Option<String>,
}

#[derive(Deserialize)]
struct Grant {
    username: String,
    credential: String,
    credential_id: Uuid,
    namespace: Option<String>,
    issuer: String,
    subject: String,
    expires_at: u64,
    actions: Vec<String>,
}

fn validate_grant(grant: &Grant, attempt: &Attempt) -> Result<(), AppError> {
    if grant.issuer != attempt.issuer
        || grant.subject != attempt.subject
        || grant.namespace.as_deref() != Some(grant.username.as_str())
        || grant.actions != ["pull"]
        || grant.credential.is_empty()
        || grant.credential.len() > 4096
        || grant.credential.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    knotree_registry::validate_registry_username(&grant.username)?;
    let now = OffsetDateTime::now_utc().unix_timestamp() as u64;
    if grant.expires_at <= now || grant.expires_at > now + 31 * 86400 {
        return Err(invalid());
    }
    Ok(())
}

/// Completes an account-level consent if `state_token` belongs to one.
/// Returns `None` so the project-level callback can handle other attempts.
pub(crate) async fn complete_callback(
    state: &AppState,
    headers: &HeaderMap,
    user_id: Uuid,
    state_token: &str,
    code: Option<String>,
    error: Option<&str>,
) -> Result<Option<Response>, AppError> {
    let config = state.config.sso.as_ref().ok_or_else(invalid)?;
    // Consumed only for the initiating user and live browser session.
    let attempt: Option<Attempt> = sqlx::query_as(
        "DELETE FROM registry_account_consent_attempts
         WHERE state_hash=$1 AND session_hash=$2 AND user_id=$3 AND issuer=$4 AND expires_at>now()
         RETURNING issuer, subject, verifier_ciphertext, return_to",
    )
    .bind(security::token_hash(state_token))
    .bind(registry_consent::session_hash(state, headers)?)
    .bind(user_id)
    .bind(&config.issuer)
    .fetch_optional(&state.db)
    .await?;
    let Some(attempt) = attempt else {
        return Ok(None);
    };
    if sso_subject(state, user_id, &attempt.issuer).await?.as_deref() != Some(attempt.subject.as_str()) {
        return Err(invalid());
    }
    let status = if error == Some("access_denied") {
        "denied"
    } else {
        if error.is_some() {
            return Err(invalid());
        }
        let code = code
            .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(invalid)?;
        let verifier = security::decrypt_secret(
            &attempt.verifier_ciphertext,
            &state.config.database_credentials_encryption_key,
        )?;
        let response = registry_consent::client()?
            .post(format!("{REGISTRY}/api/v1/cloud-grants/exchange"))
            .json(&serde_json::json!({
                "client_id": registry_consent::CLIENT,
                "redirect_uri": registry_consent::CALLBACK,
                "code": code,
                "code_verifier": verifier,
            }))
            .send()
            .await
            .map_err(|_| invalid())?;
        let grant: Grant = registry_consent::bounded_json(response).await?;
        validate_grant(&grant, &attempt)?;
        let encrypted = security::encrypt_secret(
            &grant.credential,
            &state.config.database_credentials_encryption_key,
        )?;
        let expiry =
            OffsetDateTime::from_unix_timestamp(grant.expires_at as i64).map_err(|_| invalid())?;
        store_account(state, user_id, &attempt, &grant, &encrypted, expiry).await?;
        "connected"
    };
    let mut destination = Url::parse(&config.frontend_url).map_err(|_| invalid())?;
    let path = attempt.return_to.as_deref().unwrap_or("/integrations");
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    destination.set_path(path);
    destination.set_query((!query.is_empty()).then_some(query));
    destination.query_pairs_mut().append_pair("registry", status);
    Ok(Some(no_store(Redirect::to(destination.as_str()).into_response())))
}

/// Replaces the user's account connection. Reconnecting (for example after
/// the 30-day grant expires) keeps the same account row, so every project
/// connection derived from it picks up the new credential immediately.
async fn store_account(
    state: &AppState,
    user_id: Uuid,
    attempt: &Attempt,
    grant: &Grant,
    encrypted: &str,
    expiry: OffsetDateTime,
) -> Result<(), AppError> {
    let mut transaction = state.db.begin().await?;
    let existing: Option<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id, subject, registry_username FROM knotree_registry_accounts
         WHERE user_id=$1 AND revoked_at IS NULL FOR UPDATE",
    )
    .bind(user_id)
    .fetch_optional(&mut *transaction)
    .await?;
    match existing {
        Some((id, subject, username)) if subject == attempt.subject && username == grant.username => {
            sqlx::query(
                "UPDATE knotree_registry_accounts
                 SET credential_ciphertext=$1, delegated_credential_id=$2,
                     credential_expires_at=$3, updated_at=now()
                 WHERE id=$4",
            )
            .bind(encrypted)
            .bind(grant.credential_id)
            .bind(expiry)
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        }
        existing => {
            if let Some((id, _, _)) = existing {
                revoke_account_rows(&mut transaction, id, "Knotree Registry account was replaced.").await?;
            }
            sqlx::query(
                "INSERT INTO knotree_registry_accounts
                    (id, user_id, issuer, subject, registry_username, credential_ciphertext,
                     delegated_credential_id, credential_expires_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(Uuid::new_v4())
            .bind(user_id)
            .bind(&attempt.issuer)
            .bind(&attempt.subject)
            .bind(&grant.username)
            .bind(encrypted)
            .bind(grant.credential_id)
            .bind(expiry)
            .execute(&mut *transaction)
            .await?;
        }
    }
    transaction.commit().await?;
    Ok(())
}

async fn revoke_account_rows(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    account_id: Uuid,
    reason: &str,
) -> Result<Vec<Uuid>, AppError> {
    sqlx::query("UPDATE knotree_registry_accounts SET revoked_at=now(), updated_at=now() WHERE id=$1")
        .bind(account_id)
        .execute(&mut **transaction)
        .await?;
    let connection_ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE knotree_registry_connections SET revoked_at=now(), updated_at=now()
         WHERE account_id=$1 AND revoked_at IS NULL RETURNING id",
    )
    .bind(account_id)
    .fetch_all(&mut **transaction)
    .await?;
    stop_connection_deploys(transaction, &connection_ids, reason).await
}

/// Registry reported that the user revoked a credential it issued to Cloud.
/// Revokes the account that holds it (only when the reported owner matches)
/// and any legacy repository connection, then stops their auto-deploys.
pub(crate) async fn revoke_from_registry(
    state: &AppState,
    credential_id: Uuid,
    owner: Option<(&str, &str)>,
) -> Result<(), AppError> {
    const REASON: &str = "Knotree Registry access was revoked in Registry. Reconnect to continue.";
    let mut transaction = state.db.begin().await?;
    let mut service_ids = Vec::new();
    if let Some((issuer, subject)) = owner {
        let account_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM knotree_registry_accounts
             WHERE delegated_credential_id=$1 AND issuer=$2 AND subject=$3 AND revoked_at IS NULL",
        )
        .bind(credential_id)
        .bind(issuer)
        .bind(subject)
        .fetch_all(&mut *transaction)
        .await?;
        for account_id in account_ids {
            service_ids.extend(revoke_account_rows(&mut transaction, account_id, REASON).await?);
        }
    }
    let connection_ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE knotree_registry_connections SET revoked_at=now(), updated_at=now()
         WHERE delegated_credential_id=$1 AND account_id IS NULL AND revoked_at IS NULL
         RETURNING id",
    )
    .bind(credential_id)
    .fetch_all(&mut *transaction)
    .await?;
    service_ids.extend(stop_connection_deploys(&mut transaction, &connection_ids, REASON).await?);
    transaction.commit().await?;
    remove_pull_secrets(state, service_ids).await;
    Ok(())
}

async fn remove_pull_secrets(state: &AppState, service_ids: Vec<Uuid>) {
    if !state.config.uses_kubernetes_workloads() {
        return;
    }
    for service_id in service_ids {
        if let Err(error) =
            crate::cluster_kubernetes::delete_app_image_pull_secret(&state.config, service_id).await
        {
            tracing::warn!(app_service_id = %service_id, error = %error,
                "could not remove Knotree Registry Kubernetes pull Secret");
        }
    }
}

async fn stop_connection_deploys(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    connection_ids: &[Uuid],
    reason: &str,
) -> Result<Vec<Uuid>, AppError> {
    let service_ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE project_app_services
         SET auto_deploy_enabled = FALSE, registry_connection_id = NULL,
             auto_deploy_error = $2, updated_at = now()
         WHERE registry_connection_id = ANY($1) RETURNING id",
    )
    .bind(connection_ids)
    .bind(reason)
    .fetch_all(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET status = 'failed', locked_until = NULL, last_error = $2, updated_at = now()
         WHERE app_service_id = ANY($1) AND status = 'pending'",
    )
    .bind(&service_ids)
    .bind(reason)
    .execute(&mut **transaction)
    .await?;
    Ok(service_ids)
}

#[derive(FromRow)]
struct AccountRow {
    id: Uuid,
    registry_username: String,
    credential_ciphertext: String,
    credential_expires_at: OffsetDateTime,
}

async fn active_account(state: &AppState, user_id: Uuid) -> Result<Option<AccountRow>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, registry_username, credential_ciphertext, credential_expires_at
         FROM knotree_registry_accounts WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountStatus {
    connected: bool,
    consent_ready: bool,
    auto_deploy_ready: bool,
    namespace: Option<String>,
    expires_at: Option<String>,
    expired: bool,
}

/// GET /integrations/knotree-registry
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let account = active_account(&state, user.id).await?;
    let now = OffsetDateTime::now_utc();
    let body = AccountStatus {
        connected: account.is_some(),
        consent_ready: state.config.sso.is_some(),
        auto_deploy_ready: state.config.knotree_registry_webhook_secret.is_some(),
        namespace: account.as_ref().map(|a| a.registry_username.clone()),
        expires_at: account.as_ref().and_then(|a| {
            a.credential_expires_at
                .format(&time::format_description::well_known::Rfc3339)
                .ok()
        }),
        expired: account.as_ref().is_some_and(|a| a.credential_expires_at <= now),
    };
    Ok(no_store(Json(body).into_response()))
}

/// DELETE /integrations/knotree-registry
pub async fn disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let account = active_account(&state, user.id).await?.ok_or_else(not_connected)?;
    let mut transaction = state.db.begin().await?;
    let service_ids = revoke_account_rows(
        &mut transaction,
        account.id,
        "Knotree Registry account was disconnected.",
    )
    .await?;
    transaction.commit().await?;
    remove_pull_secrets(&state, service_ids).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The signed-in user's own credential. Every picker request uses this and
/// nothing else, so a user can never list another user's images.
async fn own_credentials(
    state: &AppState,
    user_id: Uuid,
) -> Result<(AccountRow, String), AppError> {
    let account = active_account(state, user_id).await?.ok_or_else(not_connected)?;
    if account.credential_expires_at <= OffsetDateTime::now_utc() {
        return Err(AppError::Conflict {
            code: "KNOTREE_REGISTRY_ACCOUNT_EXPIRED",
            message: "Your Knotree Registry connection expired. Reconnect to continue.",
        });
    }
    let secret = security::decrypt_secret(
        &account.credential_ciphertext,
        &state.config.database_credentials_encryption_key,
    )?;
    Ok((account, secret))
}

fn registry_unavailable() -> AppError {
    AppError::ServiceUnavailable {
        code: "KNOTREE_REGISTRY_UNAVAILABLE",
        message: "Knotree Registry could not be reached. Try again.",
    }
}

async fn registry_get(path: &str, username: &str, secret: &str) -> Result<serde_json::Value, AppError> {
    let mut response = registry_consent::client()?
        .get(format!("{REGISTRY}{path}"))
        .basic_auth(username, Some(secret))
        .send()
        .await
        .map_err(|_| registry_unavailable())?;
    match response.status() {
        status if status.is_success() => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            return Err(AppError::Conflict {
                code: "KNOTREE_REGISTRY_ACCOUNT_REVOKED",
                message: "Knotree Registry rejected the connection. Reconnect your account.",
            });
        }
        StatusCode::NOT_FOUND => {
            return Err(AppError::NotFound {
                code: "KNOTREE_REGISTRY_REPOSITORY_NOT_FOUND",
                message: "The repository could not be found in your Knotree Registry namespace.",
            });
        }
        _ => return Err(registry_unavailable()),
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| registry_unavailable())? {
        if bytes.len() + chunk.len() > MAX_LIST_BYTES {
            return Err(registry_unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| registry_unavailable())
}

fn owned_repository(namespace: &str, repository: &str) -> Result<String, AppError> {
    let repository = knotree_registry::validate_registry_repository(repository)?;
    if !repository.starts_with(&format!("{namespace}/")) {
        return Err(AppError::Forbidden {
            code: "KNOTREE_REGISTRY_NOT_OWNER",
            message: "You can only use repositories in your own Knotree Registry namespace.",
        });
    }
    Ok(repository)
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RepositorySummary {
    #[serde(alias = "name")]
    name: String,
    #[serde(default, alias = "tag_count")]
    tag_count: usize,
    #[serde(default, alias = "latest_tag")]
    latest_tag: Option<String>,
    #[serde(default, alias = "latest_digest")]
    latest_digest: Option<String>,
    #[serde(default)]
    size: u64,
    #[serde(default, alias = "updated_at")]
    updated_at: Option<u64>,
}

/// GET /integrations/knotree-registry/repositories
pub async fn repositories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let (account, secret) = own_credentials(&state, user.id).await?;
    let body = registry_get(
        "/api/v1/integrations/cloud/repositories",
        &account.registry_username,
        &secret,
    )
    .await?;
    let repositories: Vec<RepositorySummary> =
        serde_json::from_value(body["repositories"].clone()).map_err(|_| registry_unavailable())?;
    // Defense in depth: never surface anything outside the user's namespace.
    let prefix = format!("{}/", account.registry_username);
    let repositories: Vec<_> = repositories
        .into_iter()
        .filter(|repository| repository.name.starts_with(&prefix))
        .collect();
    Ok(no_store(
        Json(serde_json::json!({
            "namespace": account.registry_username,
            "registryHost": knotree_registry::REGISTRY_HOST,
            "repositories": repositories,
        }))
        .into_response(),
    ))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TagSummary {
    tag: String,
    digest: String,
    #[serde(default)]
    size: u64,
    #[serde(default, alias = "created_at")]
    created_at: Option<u64>,
}

/// GET /integrations/knotree-registry/repositories/{*repository}
pub async fn repository_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository): Path<String>,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let (account, secret) = own_credentials(&state, user.id).await?;
    let repository = owned_repository(&account.registry_username, &repository)?;
    let body = registry_get(
        &format!("/api/v1/integrations/cloud/repositories/{repository}"),
        &account.registry_username,
        &secret,
    )
    .await?;
    let tags: Vec<TagSummary> =
        serde_json::from_value(body["tags"].clone()).map_err(|_| registry_unavailable())?;
    Ok(no_store(
        Json(serde_json::json!({"repository": repository, "tags": tags})).into_response(),
    ))
}

#[derive(Deserialize)]
pub struct ImportInput {
    repository: String,
}

/// POST /workspaces/{w}/projects/{p}/registry-connections/from-account
///
/// Returns a project connection for one repository of the user's own
/// namespace, creating it if needed. The existing app-service flow then takes
/// its `id` as `registryConnectionId`.
pub async fn import_into_project(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug)): Path<(String, String)>,
    Json(input): Json<ImportInput>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let (account, secret) = own_credentials(&state, user.id).await?;
    let repository = owned_repository(&account.registry_username, &input.repository)?;
    if !knotree_registry::verify_pull_access(&account.registry_username, &secret, &repository).await {
        return Err(AppError::Conflict {
            code: "KNOTREE_REGISTRY_AUTHENTICATION_FAILED",
            message: "Knotree Registry could not verify pull access to this repository.",
        });
    }
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knotree_registry_connections
         WHERE project_id=$1 AND account_id=$2 AND repository=$3 AND revoked_at IS NULL",
    )
    .bind(project_id)
    .bind(account.id)
    .bind(&repository)
    .fetch_optional(&state.db)
    .await?;
    let id = match existing {
        Some(id) => id,
        None => {
            // Membership is re-checked in the INSERT after network I/O.
            let id = Uuid::new_v4();
            let inserted = sqlx::query(
                "INSERT INTO knotree_registry_connections
                    (id, project_id, user_id, registry_username, repository,
                     credential_ciphertext, account_id)
                 SELECT $1, $2, $3, $4, $5, $6, $7
                 WHERE EXISTS (SELECT 1 FROM projects p
                     JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id
                     WHERE p.id = $2 AND wm.user_id = $3)
                 ON CONFLICT DO NOTHING",
            )
            .bind(id)
            .bind(project_id)
            .bind(user.id)
            .bind(&account.registry_username)
            .bind(&repository)
            .bind(&account.credential_ciphertext)
            .bind(account.id)
            .execute(&state.db)
            .await?;
            if inserted.rows_affected() == 1 {
                id
            } else {
                sqlx::query_scalar(
                    "SELECT id FROM knotree_registry_connections
                     WHERE project_id=$1 AND account_id=$2 AND repository=$3 AND revoked_at IS NULL",
                )
                .bind(project_id)
                .bind(account.id)
                .bind(&repository)
                .fetch_optional(&state.db)
                .await?
                .ok_or_else(invalid)?
            }
        }
    };
    Ok(no_store(
        Json(serde_json::json!({
            "id": id,
            "registryHost": knotree_registry::REGISTRY_HOST,
            "username": account.registry_username,
            "repository": repository,
        }))
        .into_response(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> Attempt {
        Attempt {
            issuer: "https://accounts.knotree.com".into(),
            subject: "alice".into(),
            verifier_ciphertext: "unused".into(),
            return_to: None,
        }
    }
    fn grant() -> Grant {
        Grant {
            username: "kt-alice".into(),
            credential: "secret-test-only".into(),
            credential_id: Uuid::new_v4(),
            namespace: Some("kt-alice".into()),
            issuer: "https://accounts.knotree.com".into(),
            subject: "alice".into(),
            expires_at: (OffsetDateTime::now_utc().unix_timestamp() + 86400) as u64,
            actions: vec!["pull".into()],
        }
    }

    #[test]
    fn namespace_grant_requires_exact_identity_own_namespace_and_pull_only() {
        let attempt = attempt();
        assert!(validate_grant(&grant(), &attempt).is_ok());
        let mut wrong = grant();
        wrong.subject = "bob".into();
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.namespace = Some("kt-bob".into());
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.namespace = None;
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.actions = vec!["pull".into(), "push".into()];
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.expires_at = 0;
        assert!(validate_grant(&wrong, &attempt).is_err());
    }

    #[test]
    fn picker_and_import_reject_other_namespaces() {
        assert!(owned_repository("kt-alice", "kt-alice/api").is_ok());
        assert!(owned_repository("kt-alice", "kt-bob/api").is_err());
        assert!(owned_repository("kt-alice", "kt-alice-evil/api").is_err());
        assert!(owned_repository("kt-alice", "kt-alice/../kt-bob/api").is_err());
    }

    #[test]
    fn return_to_stays_on_cloud() {
        assert_eq!(safe_return_to(Some("/workspace/w/project/p")).as_deref(), Some("/workspace/w/project/p"));
        assert!(safe_return_to(Some("//evil.example")).is_none());
        assert!(safe_return_to(Some("https://evil.example")).is_none());
        assert!(safe_return_to(Some("/\\evil.example")).is_none());
        assert!(safe_return_to(None).is_none());
    }

    #[tokio::test]
    async fn registry_revocation_only_revokes_the_reported_owner() {
        use crate::test_support::{seed_owner_project, test_app_state};
        let Some(state) = test_app_state().await else {
            return;
        };
        let project = seed_owner_project(&state).await;
        let issuer = "https://accounts.knotree.com";
        let subject = project.user_id.to_string();
        let credential_id = Uuid::new_v4();
        let account_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO knotree_registry_accounts
                (id, user_id, issuer, subject, registry_username, credential_ciphertext,
                 delegated_credential_id, credential_expires_at)
             VALUES ($1, $2, $3, $4, 'kt-owner', 'unused', $5, now() + interval '30 days')",
        )
        .bind(account_id)
        .bind(project.user_id)
        .bind(issuer)
        .bind(&subject)
        .bind(credential_id)
        .execute(&state.db)
        .await
        .unwrap();
        let derived = Uuid::new_v4();
        let legacy_credential = Uuid::new_v4();
        let legacy = Uuid::new_v4();
        for (id, account, delegated) in [
            (derived, Some(account_id), None),
            (legacy, None, Some(legacy_credential)),
        ] {
            sqlx::query(
                "INSERT INTO knotree_registry_connections
                    (id, project_id, user_id, registry_username, repository,
                     credential_ciphertext, account_id, delegated_credential_id)
                 VALUES ($1, $2, $3, 'kt-owner', 'kt-owner/app', 'unused', $4, $5)",
            )
            .bind(id)
            .bind(project.project_id)
            .bind(project.user_id)
            .bind(account)
            .bind(delegated)
            .execute(&state.db)
            .await
            .unwrap();
        }
        let revoked = |table: &'static str, id: Uuid| {
            let db = state.db.clone();
            async move {
                sqlx::query_scalar::<_, bool>(&format!(
                    "SELECT revoked_at IS NOT NULL FROM {table} WHERE id = $1"
                ))
                .bind(id)
                .fetch_one(&db)
                .await
                .unwrap()
            }
        };

        // A different owner cannot revoke someone else's account.
        revoke_from_registry(&state, credential_id, Some((issuer, "someone-else")))
            .await
            .unwrap();
        assert!(!revoked("knotree_registry_accounts", account_id).await);
        assert!(!revoked("knotree_registry_connections", derived).await);

        revoke_from_registry(&state, credential_id, Some((issuer, &subject)))
            .await
            .unwrap();
        assert!(revoked("knotree_registry_accounts", account_id).await);
        assert!(revoked("knotree_registry_connections", derived).await);
        assert!(!revoked("knotree_registry_connections", legacy).await);

        revoke_from_registry(&state, legacy_credential, None).await.unwrap();
        assert!(revoked("knotree_registry_connections", legacy).await);
        // Replays are harmless.
        revoke_from_registry(&state, legacy_credential, None).await.unwrap();
    }
}
