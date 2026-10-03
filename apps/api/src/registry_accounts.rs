//! Knotree Registry for the signed-in Knotree account.
//!
//! Cloud and Registry share one Knotree account, so there is nothing to
//! connect: Cloud calls Registry's cluster-only API, presents its projected
//! Kubernetes ServiceAccount token and names the account it acts for. Registry
//! answers only for that account's own namespace. Pull credentials are
//! per-repository, pull-only and renewed automatically.

use std::{path::PathBuf, sync::OnceLock, time::Duration};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{auth, error::AppError, knotree_registry, projects, security, state::AppState};

/// The only origin Cloud talks to in production: Registry's internal Service.
pub(crate) const INTERNAL_ORIGIN: &str =
    "http://registry-internal.knotree-registry.svc.cluster.local:8081";
const DEFAULT_TOKEN_FILE: &str = "/var/run/secrets/knotree-registry/token";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Renew a pull credential when it has less than this left.
const RENEW_BEFORE_DAYS: i64 = 30;
pub(crate) const RENEW_INTERVAL_SECONDS: u64 = 6 * 60 * 60;

#[derive(Clone, Debug)]
enum TokenSource {
    /// Projected ServiceAccount token, re-read on every call (the kubelet rotates it).
    File(PathBuf),
    /// Local development against a Registry started with `INTERNAL_AUTH=dev-token`.
    Static(String),
}

#[derive(Clone, Debug)]
pub(crate) struct InternalRegistry {
    origin: String,
    token: TokenSource,
}

impl InternalRegistry {
    fn from_values(
        origin: Option<&str>,
        token_file: Option<&str>,
        dev_token: Option<&str>,
        production: bool,
    ) -> Option<Self> {
        let origin = origin?.trim().trim_end_matches('/');
        if origin.is_empty() {
            return None;
        }
        let loopback = url::Url::parse(origin).ok().is_some_and(|url| {
            matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]")) && url.path() == "/"
        });
        if origin != INTERNAL_ORIGIN && (production || !loopback) {
            tracing::warn!(
                origin,
                "ignoring KNOTREE_REGISTRY_INTERNAL_ORIGIN; only Registry's internal Service is allowed"
            );
            return None;
        }
        let token = match dev_token.map(str::trim).filter(|token| !token.is_empty()) {
            Some(token) if !production => TokenSource::Static(token.to_owned()),
            Some(_) => {
                tracing::warn!("ignoring KNOTREE_REGISTRY_INTERNAL_TOKEN in production");
                TokenSource::File(PathBuf::from(token_file.unwrap_or(DEFAULT_TOKEN_FILE)))
            }
            None => TokenSource::File(PathBuf::from(token_file.unwrap_or(DEFAULT_TOKEN_FILE))),
        };
        Some(Self {
            origin: origin.to_owned(),
            token,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(origin: &str, token: &str) -> Self {
        Self {
            origin: origin.to_owned(),
            token: TokenSource::Static(token.to_owned()),
        }
    }

    fn bearer(&self) -> Result<String, AppError> {
        match &self.token {
            TokenSource::Static(token) => Ok(token.clone()),
            TokenSource::File(path) => std::fs::read_to_string(path)
                .map(|token| token.trim().to_owned())
                .map_err(|error| {
                    tracing::error!(path = %path.display(), error = %error,
                        "could not read the Knotree Registry service token");
                    unavailable()
                }),
        }
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        account: &Account,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, AppError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| unavailable())?;
        let mut request = client
            .request(method, format!("{}{path}", self.origin))
            .bearer_auth(self.bearer()?)
            .header("x-knotree-issuer", &account.issuer)
            .header("x-knotree-subject", &account.subject);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.map_err(|error| {
            tracing::error!(error = %error, "Knotree Registry internal API unreachable");
            unavailable()
        })?;
        match response.status() {
            status if status.is_success() => {}
            StatusCode::NOT_FOUND => {
                return Err(AppError::NotFound {
                    code: "KNOTREE_REGISTRY_REPOSITORY_NOT_FOUND",
                    message: "The repository could not be found in your Knotree Registry namespace.",
                });
            }
            status => {
                tracing::error!(%status, path, "Knotree Registry internal API refused the call");
                return Err(unavailable());
            }
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                return Err(unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| unavailable())
    }
}

/// Registry's internal API, when this Cloud is configured to use it.
pub(crate) fn internal_registry() -> Option<&'static InternalRegistry> {
    static REGISTRY: OnceLock<Option<InternalRegistry>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let env = |name: &str| std::env::var(name).ok();
            InternalRegistry::from_values(
                env("KNOTREE_REGISTRY_INTERNAL_ORIGIN").as_deref(),
                env("KNOTREE_REGISTRY_TOKEN_FILE").as_deref(),
                env("KNOTREE_REGISTRY_INTERNAL_TOKEN").as_deref(),
                env("APP_ENV").as_deref() == Some("production"),
            )
        })
        .as_ref()
}

fn unavailable() -> AppError {
    AppError::ServiceUnavailable {
        code: "KNOTREE_REGISTRY_UNAVAILABLE",
        message: "Knotree Registry could not be reached. Try again.",
    }
}

fn not_linked() -> AppError {
    AppError::Forbidden {
        code: "ACCOUNTS_SIGN_IN_REQUIRED",
        message: "Sign in with your Knotree account to use Knotree Registry.",
    }
}

fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

/// The Knotree account behind a Cloud user.
#[derive(Clone, Debug)]
pub(crate) struct Account {
    issuer: String,
    subject: String,
}

async fn account(state: &AppState, user_id: Uuid) -> Result<Option<Account>, AppError> {
    let Some(config) = state.config.sso.as_ref() else {
        return Ok(None);
    };
    let subject: Option<String> =
        sqlx::query_scalar("SELECT subject FROM sso_identities WHERE user_id=$1 AND issuer=$2")
            .bind(user_id)
            .bind(&config.issuer)
            .fetch_optional(&state.db)
            .await?;
    Ok(subject.map(|subject| Account {
        issuer: config.issuer.clone(),
        subject,
    }))
}

async fn require(
    state: &AppState,
    user_id: Uuid,
) -> Result<(&'static InternalRegistry, Account), AppError> {
    let registry = internal_registry().ok_or_else(unavailable)?;
    let account = account(state, user_id).await?.ok_or_else(not_linked)?;
    Ok((registry, account))
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

async fn list_repositories(
    registry: &InternalRegistry,
    account: &Account,
) -> Result<(String, Vec<RepositorySummary>), AppError> {
    let body = registry
        .call(
            reqwest::Method::GET,
            "/internal/v1/repositories",
            account,
            None,
        )
        .await?;
    let namespace = body["namespace"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(unavailable)?
        .to_owned();
    let repositories: Vec<RepositorySummary> =
        serde_json::from_value(body["repositories"].clone()).map_err(|_| unavailable())?;
    // Defense in depth: never surface anything outside the account's namespace.
    let prefix = format!("{namespace}/");
    let repositories = repositories
        .into_iter()
        .filter(|repository| repository.name.starts_with(&prefix))
        .collect();
    Ok((namespace, repositories))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountStatus {
    /// Always true for a signed-in Knotree account when Registry is reachable.
    connected: bool,
    auto_deploy_ready: bool,
    namespace: Option<String>,
}

/// GET /integrations/knotree-registry
pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let namespace = match (internal_registry(), account(&state, user.id).await?) {
        (Some(registry), Some(account)) => list_repositories(registry, &account)
            .await
            .ok()
            .map(|(namespace, _)| namespace),
        _ => None,
    };
    Ok(no_store(
        Json(AccountStatus {
            connected: namespace.is_some(),
            auto_deploy_ready: state.config.knotree_registry_webhook_secret.is_some(),
            namespace,
        })
        .into_response(),
    ))
}

/// GET /integrations/knotree-registry/repositories
pub async fn repositories(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let (registry, account) = require(&state, user.id).await?;
    let (namespace, repositories) = list_repositories(registry, &account).await?;
    Ok(no_store(
        Json(serde_json::json!({
            "namespace": namespace,
            "registryHost": knotree_registry::REGISTRY_HOST,
            "repositories": repositories,
        }))
        .into_response(),
    ))
}

/// GET /integrations/knotree-registry/repositories/{*repository}
pub async fn repository_tags(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(repository): Path<String>,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let (registry, account) = require(&state, user.id).await?;
    let repository = knotree_registry::validate_registry_repository(&repository)?;
    let body = registry
        .call(
            reqwest::Method::GET,
            &format!("/internal/v1/repositories/{repository}"),
            &account,
            None,
        )
        .await?;
    let tags: Vec<TagSummary> =
        serde_json::from_value(body["tags"].clone()).map_err(|_| unavailable())?;
    Ok(no_store(
        Json(serde_json::json!({"repository": repository, "tags": tags})).into_response(),
    ))
}

#[derive(Deserialize)]
struct PullCredential {
    username: String,
    secret: String,
    credential_id: Uuid,
    repository: String,
    expires_at: Option<i64>,
    actions: Vec<String>,
}

/// Asks Registry for a fresh pull-only credential for one repository of the
/// account, and checks Registry answered for exactly what was asked.
async fn issue_pull_credential(
    registry: &InternalRegistry,
    account: &Account,
    repository: &str,
) -> Result<PullCredential, AppError> {
    let body = registry
        .call(
            reqwest::Method::POST,
            "/internal/v1/pull-credentials",
            account,
            Some(serde_json::json!({"repository": repository})),
        )
        .await?;
    let credential: PullCredential = serde_json::from_value(body).map_err(|_| unavailable())?;
    if credential.repository != repository
        || credential.actions != ["pull"]
        || !repository.starts_with(&format!("{}/", credential.username))
        || credential.secret.is_empty()
    {
        tracing::error!(
            repository,
            "Knotree Registry returned an unexpected pull credential"
        );
        return Err(unavailable());
    }
    Ok(credential)
}

fn expiry(credential: &PullCredential) -> Option<OffsetDateTime> {
    credential
        .expires_at
        .and_then(|value| OffsetDateTime::from_unix_timestamp(value).ok())
}

#[derive(Deserialize)]
pub struct ImportInput {
    repository: String,
}

/// POST /workspaces/{w}/projects/{p}/registry-connections/from-account
///
/// Returns the project connection for one repository of the signed-in
/// account's namespace, creating it (or renewing its credential) as needed.
/// The app-service flow then takes its `id` as `registryConnectionId`.
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
    let (registry, account) = require(&state, user.id).await?;
    let repository = knotree_registry::validate_registry_repository(&input.repository)?;
    let (id, username) =
        link_repository(&state, registry, &account, project_id, user.id, &repository).await?;
    Ok(no_store(
        Json(serde_json::json!({
            "id": id,
            "registryHost": knotree_registry::REGISTRY_HOST,
            "username": username,
            "repository": repository,
        }))
        .into_response(),
    ))
}

pub(crate) async fn link_repository(
    state: &AppState,
    registry: &InternalRegistry,
    account: &Account,
    project_id: Uuid,
    user_id: Uuid,
    repository: &str,
) -> Result<(Uuid, String), AppError> {
    let credential = issue_pull_credential(registry, account, repository).await?;
    let repository = owned_repository(&credential.username, repository)?;
    let ciphertext = security::encrypt_secret(
        &credential.secret,
        &state.config.database_credentials_encryption_key,
    )?;
    let expires_at = expiry(&credential);
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knotree_registry_connections
         WHERE project_id=$1 AND repository=$2 AND owner_issuer=$3 AND owner_subject=$4
           AND revoked_at IS NULL
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(project_id)
    .bind(&repository)
    .bind(&account.issuer)
    .bind(&account.subject)
    .fetch_optional(&state.db)
    .await?;
    if let Some(id) = existing {
        sqlx::query(
            "UPDATE knotree_registry_connections
             SET registry_username=$2, credential_ciphertext=$3, delegated_credential_id=$4,
                 credential_expires_at=$5, account_id=NULL, verified_at=now(), updated_at=now()
             WHERE id=$1",
        )
        .bind(id)
        .bind(&credential.username)
        .bind(&ciphertext)
        .bind(credential.credential_id)
        .bind(expires_at)
        .execute(&state.db)
        .await?;
        return Ok((id, credential.username));
    }
    // Membership is re-checked in the INSERT after network I/O.
    let id = Uuid::new_v4();
    let inserted = sqlx::query(
        "INSERT INTO knotree_registry_connections
            (id, project_id, user_id, registry_username, repository, credential_ciphertext,
             delegated_credential_id, credential_expires_at, owner_issuer, owner_subject)
         SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10
         WHERE EXISTS (SELECT 1 FROM projects p
             JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id
             WHERE p.id = $2 AND wm.user_id = $3)",
    )
    .bind(id)
    .bind(project_id)
    .bind(user_id)
    .bind(&credential.username)
    .bind(&repository)
    .bind(&ciphertext)
    .bind(credential.credential_id)
    .bind(expires_at)
    .bind(&account.issuer)
    .bind(&account.subject)
    .execute(&state.db)
    .await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::Forbidden {
            code: "PROJECT_ACCESS_DENIED",
            message: "You no longer have access to this project.",
        });
    }
    Ok((id, credential.username))
}

/// Renews one connection's pull credential through the internal API and
/// rewrites the pull Secrets of the app services that use it.
async fn renew_connection(
    state: &AppState,
    registry: &InternalRegistry,
    connection_id: Uuid,
    account: &Account,
    repository: &str,
) -> Result<(), AppError> {
    let credential = issue_pull_credential(registry, account, repository).await?;
    let ciphertext = security::encrypt_secret(
        &credential.secret,
        &state.config.database_credentials_encryption_key,
    )?;
    sqlx::query(
        "UPDATE knotree_registry_connections
         SET registry_username=$2, credential_ciphertext=$3, delegated_credential_id=$4,
             credential_expires_at=$5, account_id=NULL, updated_at=now()
         WHERE id=$1 AND revoked_at IS NULL",
    )
    .bind(connection_id)
    .bind(&credential.username)
    .bind(&ciphertext)
    .bind(credential.credential_id)
    .bind(expiry(&credential))
    .execute(&state.db)
    .await?;
    if state.config.uses_kubernetes_workloads() {
        let services: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM project_app_services WHERE registry_connection_id=$1",
        )
        .bind(connection_id)
        .fetch_all(&state.db)
        .await?;
        for service_id in services {
            if let Err(error) = crate::cluster_kubernetes::update_app_image_pull_secret(
                &state.config,
                service_id,
                &credential.username,
                &credential.secret,
            )
            .await
            {
                tracing::warn!(app_service_id = %service_id, error = %error,
                    "could not update the Knotree Registry pull Secret");
            }
        }
    }
    Ok(())
}

/// Connections whose credential needs renewing: close to expiry, already
/// expired, or still on a credential from the retired consent flow.
async fn renewable_connections(
    state: &AppState,
    only: Option<Uuid>,
) -> Result<Vec<(Uuid, String, String, String)>, AppError> {
    Ok(sqlx::query_as(
        "SELECT id, owner_issuer, owner_subject, repository
         FROM knotree_registry_connections
         WHERE revoked_at IS NULL AND owner_issuer IS NOT NULL AND owner_subject IS NOT NULL
           AND ($1::uuid IS NULL OR id = $1)
           AND (account_id IS NOT NULL
                OR credential_expires_at IS NULL
                OR credential_expires_at < now() + make_interval(days => $2::int))",
    )
    .bind(only)
    .bind(RENEW_BEFORE_DAYS as i32)
    .fetch_all(&state.db)
    .await?)
}

/// Renews the connection's credential if it is close to expiry. Deploys call
/// this first, so a deploy never fails on a credential Cloud could renew.
pub(crate) async fn renew_if_needed(state: &AppState, connection_id: Uuid) {
    let Some(registry) = internal_registry() else {
        return;
    };
    let rows = match renewable_connections(state, Some(connection_id)).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(error = ?error, "could not check the Knotree Registry credential");
            return;
        }
    };
    for (id, issuer, subject, repository) in rows {
        let account = Account { issuer, subject };
        if let Err(error) = renew_connection(state, registry, id, &account, &repository).await {
            tracing::warn!(registry_connection_id = %id, error = ?error,
                "could not renew the Knotree Registry pull credential");
        }
    }
}

/// Background renewal for every connection, so running apps keep pulling.
pub(crate) async fn renew_expiring_credentials(state: &AppState) {
    let Some(registry) = internal_registry() else {
        return;
    };
    let rows = match renewable_connections(state, None).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(error = ?error, "could not list Knotree Registry credentials to renew");
            return;
        }
    };
    for (id, issuer, subject, repository) in rows {
        let account = Account { issuer, subject };
        if let Err(error) = renew_connection(state, registry, id, &account, &repository).await {
            tracing::warn!(registry_connection_id = %id, error = ?error,
                "could not renew the Knotree Registry pull credential");
        }
    }
}

/// Registry reported that a credential it issued to Cloud through the retired
/// consent flow was revoked. Stops auto-deploy for connections that still use
/// it; connections already renewed through the internal API are unaffected.
pub(crate) async fn revoke_from_registry(
    state: &AppState,
    credential_id: Uuid,
    owner: Option<(&str, &str)>,
) -> Result<(), AppError> {
    const REASON: &str = "Knotree Registry access was revoked in Registry.";
    let mut transaction = state.db.begin().await?;
    let mut connection_ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE knotree_registry_connections SET revoked_at=now(), updated_at=now()
         WHERE delegated_credential_id=$1 AND account_id IS NULL AND revoked_at IS NULL
         RETURNING id",
    )
    .bind(credential_id)
    .fetch_all(&mut *transaction)
    .await?;
    if let Some((issuer, subject)) = owner {
        let account_ids: Vec<Uuid> = sqlx::query_scalar(
            "UPDATE knotree_registry_accounts SET revoked_at=now(), updated_at=now()
             WHERE delegated_credential_id=$1 AND issuer=$2 AND subject=$3 AND revoked_at IS NULL
             RETURNING id",
        )
        .bind(credential_id)
        .bind(issuer)
        .bind(subject)
        .fetch_all(&mut *transaction)
        .await?;
        connection_ids.extend(
            sqlx::query_scalar::<_, Uuid>(
                "UPDATE knotree_registry_connections SET revoked_at=now(), updated_at=now()
                 WHERE account_id = ANY($1) AND revoked_at IS NULL RETURNING id",
            )
            .bind(&account_ids)
            .fetch_all(&mut *transaction)
            .await?,
        );
    }
    let service_ids: Vec<Uuid> = sqlx::query_scalar(
        "UPDATE project_app_services
         SET auto_deploy_enabled = FALSE, registry_connection_id = NULL,
             auto_deploy_error = $2, updated_at = now()
         WHERE registry_connection_id = ANY($1) RETURNING id",
    )
    .bind(&connection_ids)
    .bind(REASON)
    .fetch_all(&mut *transaction)
    .await?;
    sqlx::query(
        "UPDATE knotree_registry_deploy_jobs
         SET status = 'failed', locked_until = NULL, last_error = $2, updated_at = now()
         WHERE app_service_id = ANY($1) AND status = 'pending'",
    )
    .bind(&service_ids)
    .bind(REASON)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if state.config.uses_kubernetes_workloads() {
        for service_id in service_ids {
            if let Err(error) =
                crate::cluster_kubernetes::delete_app_image_pull_secret(&state.config, service_id)
                    .await
            {
                tracing::warn!(app_service_id = %service_id, error = %error,
                    "could not remove Knotree Registry Kubernetes pull Secret");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get, routing::post};

    #[test]
    fn only_the_internal_service_is_trusted_in_production() {
        let configured = |origin: &str, production: bool| {
            InternalRegistry::from_values(Some(origin), None, None, production).is_some()
        };
        assert!(configured(INTERNAL_ORIGIN, true));
        assert!(!configured("https://registry.knotree.com", true));
        assert!(!configured("http://127.0.0.1:8081", true));
        assert!(configured("http://127.0.0.1:8081", false));
        assert!(!configured("http://attacker.example:8081", false));
        assert!(InternalRegistry::from_values(None, None, None, true).is_none());
        // The dev token never replaces the ServiceAccount token in production.
        let production =
            InternalRegistry::from_values(Some(INTERNAL_ORIGIN), None, Some("dev"), true).unwrap();
        assert!(matches!(production.token, TokenSource::File(_)));
    }

    #[test]
    fn imports_reject_other_namespaces() {
        assert!(owned_repository("kt-alice", "kt-alice/api").is_ok());
        assert!(owned_repository("kt-alice", "kt-bob/api").is_err());
        assert!(owned_repository("kt-alice", "kt-alice-evil/api").is_err());
        assert!(owned_repository("kt-alice", "kt-alice/../kt-bob/api").is_err());
    }

    /// A stand-in for Registry's internal API that records who Cloud acted for.
    async fn fake_registry() -> String {
        async fn repositories(headers: HeaderMap) -> Json<serde_json::Value> {
            assert_eq!(headers["authorization"], "Bearer test-internal-token");
            let subject = headers["x-knotree-subject"].to_str().unwrap();
            Json(serde_json::json!({
                "namespace": format!("kt-{subject}"),
                "repositories": [
                    {"name": format!("kt-{subject}/api"), "tag_count": 1},
                    {"name": "kt-someone-else/api", "tag_count": 1},
                ],
            }))
        }
        async fn credential(headers: HeaderMap, Json(body): Json<serde_json::Value>) -> Response {
            let subject = headers["x-knotree-subject"].to_str().unwrap();
            let repository = body["repository"].as_str().unwrap();
            if !repository.starts_with(&format!("kt-{subject}/")) {
                return StatusCode::NOT_FOUND.into_response();
            }
            Json(serde_json::json!({
                "username": format!("kt-{subject}"),
                "secret": format!("secret-{}", Uuid::new_v4()),
                "credential_id": Uuid::new_v4(),
                "repository": repository,
                "expires_at": OffsetDateTime::now_utc().unix_timestamp() + 90 * 86400,
                "actions": ["pull"],
            }))
            .into_response()
        }
        let app = Router::new()
            .route("/internal/v1/repositories", get(repositories))
            .route("/internal/v1/pull-credentials", post(credential));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn links_and_renews_only_the_accounts_own_repositories() {
        use crate::test_support::{seed_owner_project, test_app_state};
        let Some(state) = test_app_state().await else {
            return;
        };
        let registry = InternalRegistry::for_test(&fake_registry().await, "test-internal-token");
        let project = seed_owner_project(&state).await;
        let alice = Account {
            issuer: "https://accounts.knotree.com".into(),
            subject: format!("alice{}", Uuid::new_v4().simple()),
        };

        let (namespace, repositories) = list_repositories(&registry, &alice).await.unwrap();
        assert_eq!(namespace, format!("kt-{}", alice.subject));
        assert_eq!(repositories.len(), 1, "other namespaces are filtered out");

        let repository = format!("{namespace}/api");
        let (id, username) = link_repository(
            &state,
            &registry,
            &alice,
            project.project_id,
            project.user_id,
            &repository,
        )
        .await
        .unwrap();
        assert_eq!(username, namespace);
        let stored: (String, Option<String>) = sqlx::query_as(
            "SELECT credential_ciphertext, owner_subject FROM knotree_registry_connections WHERE id=$1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(stored.1.as_deref(), Some(alice.subject.as_str()));

        // Linking again reuses the connection and renews its credential.
        let (again, _) = link_repository(
            &state,
            &registry,
            &alice,
            project.project_id,
            project.user_id,
            &repository,
        )
        .await
        .unwrap();
        assert_eq!(again, id);
        let renewed: String = sqlx::query_scalar(
            "SELECT credential_ciphertext FROM knotree_registry_connections WHERE id=$1",
        )
        .bind(id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_ne!(renewed, stored.0);

        // Someone else's repository is refused by Registry.
        let other = link_repository(
            &state,
            &registry,
            &alice,
            project.project_id,
            project.user_id,
            "kt-bob/api",
        )
        .await;
        assert!(other.is_err());

        // A credential close to expiry is renewed by the background job.
        sqlx::query(
            "UPDATE knotree_registry_connections SET credential_expires_at = now() + interval '1 day' WHERE id=$1",
        )
        .bind(id)
        .execute(&state.db)
        .await
        .unwrap();
        let due = renewable_connections(&state, Some(id)).await.unwrap();
        assert_eq!(due.len(), 1);
        renew_connection(&state, &registry, id, &alice, &repository)
            .await
            .unwrap();
        assert!(
            renewable_connections(&state, Some(id))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
