use std::time::Duration;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sqlx::FromRow;
use time::OffsetDateTime;
use url::Url;
use uuid::Uuid;

use crate::{auth, error::AppError, knotree_registry, projects, security, state::AppState};

const REGISTRY: &str = "https://registry.knotree.com";
pub(crate) const CLIENT: &str = "knotree-cloud";
pub(crate) const CALLBACK: &str = "https://cloud.knotree.com/api/v1/auth/knotree-registry/callback";

fn invalid() -> AppError {
    AppError::BadRequest {
        code: "REGISTRY_CONSENT_FAILED",
        message: "Registry authorization expired or could not be verified. Start again.",
    }
}
pub(crate) fn session_hash(state: &AppState, headers: &HeaderMap) -> Result<Vec<u8>, AppError> {
    let token =
        security::get_cookie(headers, state.config.session_cookie_name()).ok_or_else(invalid)?;
    Ok(security::token_hash(&token))
}
pub(crate) fn registry_consent_unreachable(error: reqwest::Error) -> AppError {
    tracing::warn!(error = %error, "could not reach Knotree Registry for consent");
    invalid()
}
pub(crate) fn client() -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| invalid())
}
pub(crate) async fn bounded_json<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, AppError> {
    if !response.status().is_success() {
        tracing::warn!(status = %response.status(), url = %response.url(),
            "Knotree Registry rejected a consent request");
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| invalid())? {
        if bytes.len() + chunk.len() > 16 * 1024 {
            return Err(invalid());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}
#[derive(Deserialize)]
pub struct StartInput {
    repository: String,
}
#[derive(Deserialize)]
struct Started {
    request_id: Uuid,
    authorization_url: String,
}

pub async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug)): Path<(String, String)>,
    Json(input): Json<StartInput>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let config = state
        .config
        .sso
        .as_ref()
        .ok_or(AppError::ServiceUnavailable {
            code: "SSO_NOT_CONFIGURED",
            message: "Sign in with Knotree Accounts to connect Registry.",
        })?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_id, &project_slug).await?;
    let repository = knotree_registry::validate_registry_repository(&input.repository)?;
    let subject: String = sqlx::query_scalar("SELECT subject FROM sso_identities WHERE user_id=$1 AND issuer=$2")
        .bind(user.id).bind(&config.issuer).fetch_optional(&state.db).await?
        .ok_or(AppError::Forbidden { code: "ACCOUNTS_SIGN_IN_REQUIRED", message: "This Cloud account must be linked to Knotree Accounts before connecting Registry." })?;
    let state_token = security::random_token();
    let verifier = security::random_token();
    let challenge = URL_SAFE_NO_PAD.encode(security::token_hash(&verifier));
    let response = client()?.post(format!("{}/api/v1/cloud-grants/requests", crate::knotree_registry::registry_api_origin()))
        .json(&serde_json::json!({"client_id":CLIENT,"redirect_uri":CALLBACK,"state":state_token,"repository":repository,"code_challenge":challenge,"code_challenge_method":"S256","expected_issuer":config.issuer,"expected_subject":subject}))
        .send().await.map_err(registry_consent_unreachable)?;
    let started: Started = bounded_json(response).await?;
    if started.authorization_url != format!("{REGISTRY}/cloud/authorize/{}", started.request_id) {
        return Err(invalid());
    }
    sqlx::query("DELETE FROM registry_consent_attempts WHERE expires_at <= now() OR (user_id=$1 AND project_id=$2)")
        .bind(user.id).bind(project_id).execute(&state.db).await?;
    sqlx::query("INSERT INTO registry_consent_attempts (state_hash,session_hash,user_id,project_id,workspace_id,project_slug,repository,issuer,subject,verifier_ciphertext,expires_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,now()+interval '10 minutes')")
        .bind(security::token_hash(&state_token)).bind(session_hash(&state,&headers)?).bind(user.id).bind(project_id)
        .bind(workspace_id).bind(project_slug).bind(repository).bind(&config.issuer).bind(subject)
        .bind(security::encrypt_secret(&verifier,&state.config.database_credentials_encryption_key)?)
        .execute(&state.db).await?;
    let mut response =
        Json(serde_json::json!({"authorizationUrl":started.authorization_url})).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    Ok(response)
}

#[derive(Deserialize)]
pub struct Callback {
    state: Option<String>,
    code: Option<String>,
    error: Option<String>,
}
#[derive(FromRow)]
struct Attempt {
    project_id: Uuid,
    workspace_id: String,
    project_slug: String,
    repository: String,
    issuer: String,
    subject: String,
    verifier_ciphertext: String,
}
#[derive(Deserialize)]
struct Grant {
    username: String,
    credential: String,
    credential_id: Uuid,
    repository: String,
    issuer: String,
    subject: String,
    expires_at: u64,
    actions: Vec<String>,
}
fn validate_grant(grant: &Grant, attempt: &Attempt) -> Result<(), AppError> {
    if grant.issuer != attempt.issuer
        || grant.subject != attempt.subject
        || grant.repository != attempt.repository
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
pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<Callback>,
) -> Result<Response, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let config = state.config.sso.as_ref().ok_or_else(invalid)?;
    let token = query.state.filter(|v| v.len() == 43).ok_or_else(invalid)?;
    // Account-level ("connect once") consents share this registered callback.
    if let Some(response) = crate::registry_accounts::complete_callback(
        &state,
        &headers,
        user.id,
        &token,
        query.code.clone(),
        query.error.as_deref(),
    )
    .await?
    {
        return Ok(response);
    }
    // Consume only for the initiating user and live browser session. Replays,
    // another signed-in account, and configuration changes fail closed.
    let attempt: Attempt = sqlx::query_as("DELETE FROM registry_consent_attempts WHERE state_hash=$1 AND session_hash=$2 AND user_id=$3 AND issuer=$4 AND expires_at>now() RETURNING project_id,workspace_id,project_slug,repository,issuer,subject,verifier_ciphertext")
        .bind(security::token_hash(&token)).bind(session_hash(&state,&headers)?).bind(user.id).bind(&config.issuer)
        .fetch_optional(&state.db).await?.ok_or_else(invalid)?;
    let project_id = projects::accessible_project_id(
        &state,
        user.id,
        &attempt.workspace_id,
        &attempt.project_slug,
    )
    .await?;
    if project_id != attempt.project_id {
        return Err(invalid());
    }
    let subject: Option<String> =
        sqlx::query_scalar("SELECT subject FROM sso_identities WHERE user_id=$1 AND issuer=$2")
            .bind(user.id)
            .bind(&attempt.issuer)
            .fetch_optional(&state.db)
            .await?;
    if subject.as_deref() != Some(attempt.subject.as_str()) {
        return Err(invalid());
    }
    let status = if query.error.as_deref() == Some("access_denied") {
        "denied"
    } else {
        if query.error.is_some() {
            return Err(invalid());
        }
        let code = query
            .code
            .filter(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(invalid)?;
        let verifier = security::decrypt_secret(
            &attempt.verifier_ciphertext,
            &state.config.database_credentials_encryption_key,
        )?;
        let response = client()?.post(format!("{}/api/v1/cloud-grants/exchange", crate::knotree_registry::registry_api_origin()))
            .json(&serde_json::json!({"client_id":CLIENT,"redirect_uri":CALLBACK,"code":code,"code_verifier":verifier}))
            .send().await.map_err(registry_consent_unreachable)?;
        let grant: Grant = bounded_json(response).await?;
        validate_grant(&grant, &attempt)?;
        if !knotree_registry::verify_pull_access(
            &grant.username,
            &grant.credential,
            &attempt.repository,
        )
        .await
        {
            return Err(invalid());
        }
        let encrypted = security::encrypt_secret(
            &grant.credential,
            &state.config.database_credentials_encryption_key,
        )?;
        let expiry =
            OffsetDateTime::from_unix_timestamp(grant.expires_at as i64).map_err(|_| invalid())?;
        // Membership is checked again in the INSERT, after network I/O, to
        // prevent connecting a project whose access was just removed.
        let result = sqlx::query("INSERT INTO knotree_registry_connections (id,project_id,user_id,registry_username,repository,credential_ciphertext,delegated_credential_id,credential_expires_at) SELECT $1,$2,$3,$4,$5,$6,$7,$8 WHERE EXISTS (SELECT 1 FROM projects p JOIN workspace_memberships wm ON wm.workspace_id=p.workspace_id WHERE p.id=$2 AND wm.user_id=$3)")
            .bind(Uuid::new_v4()).bind(project_id).bind(user.id).bind(grant.username).bind(attempt.repository)
            .bind(encrypted).bind(grant.credential_id).bind(expiry).execute(&state.db).await?;
        if result.rows_affected() != 1 {
            return Err(invalid());
        }
        "connected"
    };
    let mut destination = Url::parse(&config.frontend_url).map_err(|_| invalid())?;
    destination
        .path_segments_mut()
        .map_err(|_| invalid())?
        .clear()
        .extend([
            "workspace",
            &attempt.workspace_id,
            "project",
            &attempt.project_slug,
        ]);
    destination
        .query_pairs_mut()
        .append_pair("registry", status);
    let mut response = Redirect::to(destination.as_str()).into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> Attempt {
        Attempt {
            project_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4().to_string(),
            project_slug: "app".into(),
            repository: "kt-owner/app".into(),
            issuer: "https://accounts.knotree.com".into(),
            subject: "alice".into(),
            verifier_ciphertext: "unused".into(),
        }
    }
    fn grant() -> Grant {
        Grant {
            username: "kt-owner".into(),
            credential: "secret-test-only".into(),
            credential_id: Uuid::new_v4(),
            repository: "kt-owner/app".into(),
            issuer: "https://accounts.knotree.com".into(),
            subject: "alice".into(),
            expires_at: (OffsetDateTime::now_utc().unix_timestamp() + 86400) as u64,
            actions: vec!["pull".into()],
        }
    }
    #[test]
    fn returned_grant_requires_exact_identity_repository_and_pull_only() {
        let attempt = attempt();
        assert!(validate_grant(&grant(), &attempt).is_ok());
        let mut wrong = grant();
        wrong.subject = "bob".into();
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.issuer = "https://attacker.example".into();
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.repository = "other/app".into();
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.actions.push("push".into());
        assert!(validate_grant(&wrong, &attempt).is_err());
        wrong = grant();
        wrong.expires_at = 0;
        assert!(validate_grant(&wrong, &attempt).is_err());
    }

    #[tokio::test]
    async fn denied_callback_is_bound_to_session_and_consumed_once() {
        use crate::test_support::{seed_owner_project, session_headers, test_app_state_configured};
        let Some(state) = test_app_state_configured(|config| {
            config.sso = Some(crate::sso::SsoConfig {
                issuer: "https://accounts.knotree.com".into(),
                service_origin: "https://accounts.knotree.com".into(),
                client_id: CLIENT.into(),
                redirect_uri: "https://cloud.knotree.com/api/v1/auth/sso/callback".into(),
                frontend_url: "https://cloud.knotree.com".into(),
            });
        })
        .await
        else {
            return;
        };
        let project = seed_owner_project(&state).await;
        let headers = session_headers(&state, project.user_id).await;
        let other_session = session_headers(&state, project.user_id).await;
        sqlx::query("INSERT INTO sso_identities (issuer,subject,user_id) VALUES ($1,$2,$3)")
            .bind("https://accounts.knotree.com")
            .bind(project.user_id.to_string())
            .bind(project.user_id)
            .execute(&state.db)
            .await
            .unwrap();
        let token = security::random_token();
        sqlx::query("INSERT INTO registry_consent_attempts (state_hash,session_hash,user_id,project_id,workspace_id,project_slug,repository,issuer,subject,verifier_ciphertext,expires_at) VALUES ($1,$2,$3,$4,$5,$6,'kt-owner/app','https://accounts.knotree.com',$7,'unused',now()+interval '10 minutes')")
            .bind(security::token_hash(&token)).bind(session_hash(&state,&headers).unwrap())
            .bind(project.user_id).bind(project.project_id).bind(&project.workspace_route_id)
            .bind(&project.project_slug).bind(project.user_id.to_string()).execute(&state.db).await.unwrap();
        let denied = || {
            Query(Callback {
                state: Some(token.clone()),
                code: None,
                error: Some("access_denied".into()),
            })
        };
        assert!(
            callback(State(state.clone()), other_session, denied())
                .await
                .is_err()
        );
        let response = callback(State(state.clone()), headers.clone(), denied())
            .await
            .unwrap();
        let location = response.headers()["location"].to_str().unwrap();
        assert!(location.ends_with("?registry=denied"));
        assert!(!location.contains(&token));
        assert!(
            callback(State(state.clone()), headers, denied())
                .await
                .is_err()
        );
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM knotree_registry_connections WHERE project_id=$1",
        )
        .bind(project.project_id)
        .fetch_one(&state.db)
        .await
        .unwrap();
        assert_eq!(count, 0);
    }
}
