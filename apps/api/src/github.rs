use std::time::Duration as StdDuration;

use anyhow::{Context, Result, bail};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Redirect, Response},
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use url::Url;
use uuid::Uuid;

use crate::{auth, error::AppError, security, state::AppState};

const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";
const GITHUB_OAUTH_SCOPE: &str = "read:packages repo";
const OAUTH_STATE_TTL_MINUTES: i64 = 10;

#[derive(Debug, Deserialize)]
pub struct GithubStartQuery {
    #[serde(rename = "returnTo")]
    pub return_to: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubAuthorizationResponse {
    pub authorization_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubConnectionStatusResponse {
    pub connected: bool,
    pub login: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GithubCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct GithubDockerCredentials {
    pub login: String,
    pub access_token: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GithubConnectionRow {
    github_login: String,
    access_token_ciphertext: String,
}

#[derive(Debug, sqlx::FromRow)]
struct GithubOAuthStateRow {
    user_id: Uuid,
    return_to: String,
}

pub async fn start(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<GithubStartQuery>,
) -> Result<Json<GithubAuthorizationResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let client_id =
        state
            .config
            .github_client_id
            .as_deref()
            .ok_or(AppError::ServiceUnavailable {
                code: "GITHUB_OAUTH_NOT_CONFIGURED",
                message: "GitHub login is not configured for this environment.",
            })?;

    let oauth_state = security::random_token();
    let return_to = safe_return_to(query.return_to.as_deref());
    sqlx::query("DELETE FROM github_oauth_states WHERE user_id = $1 OR expires_at <= now()")
        .bind(user.id)
        .execute(&state.db)
        .await?;
    sqlx::query(
        "INSERT INTO github_oauth_states (state_hash, user_id, return_to, expires_at) VALUES ($1, $2, $3, $4)",
    )
    .bind(security::token_hash(&oauth_state))
    .bind(user.id)
    .bind(return_to)
    .bind(OffsetDateTime::now_utc() + Duration::minutes(OAUTH_STATE_TTL_MINUTES))
    .execute(&state.db)
    .await?;

    let mut authorization_url = Url::parse(GITHUB_AUTHORIZE_URL)
        .map_err(|error| AppError::internal(format!("invalid GitHub authorize URL: {error}")))?;
    authorization_url
        .query_pairs_mut()
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", &state.config.github_oauth_redirect_uri)
        .append_pair("scope", GITHUB_OAUTH_SCOPE)
        .append_pair("prompt", "consent")
        .append_pair("state", &oauth_state);

    Ok(Json(GithubAuthorizationResponse {
        authorization_url: authorization_url.into(),
    }))
}

pub async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<GithubConnectionStatusResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let connection = sqlx::query_as::<_, GithubConnectionRow>(
        "SELECT github_login, access_token_ciphertext FROM github_connections WHERE user_id = $1",
    )
    .bind(user.id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(GithubConnectionStatusResponse {
        connected: connection.is_some(),
        login: connection.map(|connection| connection.github_login),
    }))
}

pub async fn disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    sqlx::query("DELETE FROM github_connections WHERE user_id = $1")
        .bind(user.id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn callback(
    State(state): State<AppState>,
    Query(query): Query<GithubCallbackQuery>,
) -> Response {
    let Some(oauth_state) = query.state.as_deref().filter(|value| !value.is_empty()) else {
        return callback_redirect(&state, "/", "error");
    };

    let state_row = match consume_oauth_state(&state, oauth_state).await {
        Ok(Some(row)) => row,
        Ok(None) => return callback_redirect(&state, "/", "error"),
        Err(error) => {
            tracing::error!(error = %error, "could not consume GitHub OAuth state");
            return callback_redirect(&state, "/", "error");
        }
    };

    if query.error.is_some() {
        return callback_redirect(&state, &state_row.return_to, "error");
    }

    let Some(code) = query.code.as_deref().filter(|value| !value.is_empty()) else {
        return callback_redirect(&state, &state_row.return_to, "error");
    };

    match finish_github_login(&state, state_row.user_id, code).await {
        Ok(()) => callback_redirect(&state, &state_row.return_to, "connected"),
        Err(error) => {
            tracing::warn!(error = %error, user_id = %state_row.user_id, "GitHub OAuth login failed");
            callback_redirect(&state, &state_row.return_to, "error")
        }
    }
}

pub(crate) async fn docker_credentials(
    state: &AppState,
    user_id: Uuid,
) -> Result<Option<GithubDockerCredentials>, AppError> {
    let connection = sqlx::query_as::<_, GithubConnectionRow>(
        "SELECT github_login, access_token_ciphertext FROM github_connections WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    connection
        .map(|connection| {
            Ok(GithubDockerCredentials {
                login: connection.github_login,
                access_token: security::decrypt_secret(
                    &connection.access_token_ciphertext,
                    &state.config.database_credentials_encryption_key,
                )?,
            })
        })
        .transpose()
}

async fn consume_oauth_state(
    state: &AppState,
    oauth_state: &str,
) -> Result<Option<GithubOAuthStateRow>, sqlx::Error> {
    sqlx::query_as::<_, GithubOAuthStateRow>(
        "DELETE FROM github_oauth_states WHERE state_hash = $1 AND expires_at > now() RETURNING user_id, return_to",
    )
    .bind(security::token_hash(oauth_state))
    .fetch_optional(&state.db)
    .await
}

async fn finish_github_login(state: &AppState, user_id: Uuid, code: &str) -> Result<()> {
    let client_id = state
        .config
        .github_client_id
        .as_deref()
        .context("GitHub OAuth client ID is not configured")?;
    let client_secret = state
        .config
        .github_client_secret
        .as_deref()
        .context("GitHub OAuth client secret is not configured")?;
    let client = reqwest::Client::builder()
        .timeout(StdDuration::from_secs(15))
        .user_agent("Knotree Cloud")
        .build()?;

    let token_response = client
        .post(GITHUB_ACCESS_TOKEN_URL)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            (
                "redirect_uri",
                state.config.github_oauth_redirect_uri.as_str(),
            ),
        ])
        .send()
        .await?;
    if !token_response.status().is_success() {
        bail!(
            "GitHub token exchange returned HTTP {}",
            token_response.status()
        );
    }
    let token_payload = token_response.json::<GithubTokenResponse>().await?;
    let access_token = token_payload
        .access_token
        .filter(|value| !value.is_empty())
        .context("GitHub did not return an access token")?;

    let user_response = client
        .get(GITHUB_USER_URL)
        .bearer_auth(&access_token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    if !user_response.status().is_success() {
        bail!(
            "GitHub user lookup returned HTTP {}",
            user_response.status()
        );
    }
    let github_user = user_response.json::<GithubUserResponse>().await?;
    let access_token_ciphertext = security::encrypt_secret(
        &access_token,
        &state.config.database_credentials_encryption_key,
    )
    .map_err(|_| anyhow::anyhow!("credential encryption failed"))?;

    sqlx::query(
        "INSERT INTO github_connections (user_id, github_user_id, github_login, access_token_ciphertext) VALUES ($1, $2, $3, $4) ON CONFLICT (user_id) DO UPDATE SET github_user_id = EXCLUDED.github_user_id, github_login = EXCLUDED.github_login, access_token_ciphertext = EXCLUDED.access_token_ciphertext, updated_at = now()",
    )
    .bind(user_id)
    .bind(github_user.id.to_string())
    .bind(github_user.login)
    .bind(access_token_ciphertext)
    .execute(&state.db)
    .await?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct GithubTokenResponse {
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubUserResponse {
    id: i64,
    login: String,
}

fn callback_redirect(state: &AppState, return_to: &str, status: &str) -> Response {
    let origin = state
        .config
        .allowed_origins
        .first()
        .map(String::as_str)
        .unwrap_or("/");
    let path = safe_return_to(Some(return_to));
    let separator = if path.contains('?') { '&' } else { '?' };
    let destination = format!("{origin}{path}{separator}github={status}");
    Redirect::temporary(&destination).into_response()
}

fn safe_return_to(value: Option<&str>) -> String {
    let Some(value) = value
        .map(str::trim)
        .filter(|value| !value.is_empty() && value.len() <= 1024)
    else {
        return "/".to_owned();
    };
    if value.starts_with('/')
        && !value.starts_with("//")
        && !value.contains("\\")
        && !value.contains("://")
        && !value.contains('#')
    {
        value.to_owned()
    } else {
        "/".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_allows_relative_callback_paths() {
        assert_eq!(
            safe_return_to(Some("/workspace/acme/project/app")),
            "/workspace/acme/project/app"
        );
        assert_eq!(safe_return_to(Some("https://evil.example")), "/");
        assert_eq!(safe_return_to(Some("//evil.example")), "/");
    }

    #[test]
    fn html_pages_require_repo_scope_on_reconnect() {
        assert!(GITHUB_OAUTH_SCOPE.split_whitespace().any(|scope| scope == "repo"));
        assert!(GITHUB_OAUTH_SCOPE.contains("read:packages"));
    }
}
