use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use sqlx::{Postgres, Transaction};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    error::AppError,
    models::{AuthResponse, CsrfResponse, UserResponse, WorkspaceResponse},
    security,
    state::AppState,
};

const SESSION_REFRESH_THRESHOLD_DAYS: i64 = 7;

#[derive(Debug, sqlx::FromRow)]
struct UserRow {
    id: Uuid,
    full_name: String,
    email: String,
    email_verified_at: Option<OffsetDateTime>,
    username: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct SessionRow {
    user_id: Uuid,
    expires_at: OffsetDateTime,
}

#[derive(Debug, sqlx::FromRow)]
struct WorkspaceRow {
    id: Uuid,
    name: String,
}

#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub id: Uuid,
}

pub async fn csrf(State(state): State<AppState>, headers: HeaderMap) -> Response {
    // Reuse the browser's current token so another tab fetching /auth/csrf
    // does not invalidate the token this tab already holds.
    let token = security::get_cookie(&headers, state.config.csrf_cookie_name())
        .filter(|value| {
            (32..=128).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
        .unwrap_or_else(security::random_token);
    let mut response = (
        StatusCode::OK,
        Json(CsrfResponse {
            csrf_token: token.clone(),
        }),
    )
        .into_response();
    security::append_cookie(
        &mut response,
        security::cookie_header(
            state.config.csrf_cookie_name(),
            &token,
            state.config.session_ttl_days * 86_400,
            true,
            state.config.cookie_secure,
        ),
    );
    response
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AuthResponse>, AppError> {
    let authenticated = authenticate(&state, &headers).await?;
    let user = fetch_user(&state, authenticated.id).await?;
    Ok(Json(AuthResponse {
        user: user_response(&user),
        workspace: fetch_workspace(&state, authenticated.id).await?,
    }))
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    if let Some(token) = security::get_cookie(&headers, state.config.session_cookie_name()) {
        sqlx::query("UPDATE sessions SET revoked_at = now() WHERE token_hash = $1")
            .bind(security::token_hash(&token))
            .execute(&state.db)
            .await?;
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    security::append_cookie(
        &mut response,
        security::cookie_header(
            state.config.session_cookie_name(),
            "",
            0,
            true,
            state.config.cookie_secure,
        ),
    );
    security::append_cookie(
        &mut response,
        security::cookie_header(
            state.config.csrf_cookie_name(),
            "",
            0,
            true,
            state.config.cookie_secure,
        ),
    );
    Ok(response)
}

pub async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthenticatedUser, AppError> {
    let token = security::get_cookie(headers, state.config.session_cookie_name()).ok_or(
        AppError::Unauthorized {
            code: "AUTHENTICATION_REQUIRED",
            message: "Authentication is required.",
        },
    )?;
    let token_hash = security::token_hash(&token);
    let session = sqlx::query_as::<_, SessionRow>(
        "SELECT user_id, expires_at FROM sessions WHERE token_hash = $1 AND revoked_at IS NULL AND expires_at > now()",
    )
    .bind(token_hash)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized {
        code: "AUTHENTICATION_REQUIRED",
        message: "Authentication is required.",
    })?;

    let now = OffsetDateTime::now_utc();
    if session.expires_at - now < Duration::days(SESSION_REFRESH_THRESHOLD_DAYS) {
        let next_expiry = now + Duration::days(state.config.session_ttl_days);
        sqlx::query(
            "UPDATE sessions SET expires_at = $1, last_used_at = now() WHERE token_hash = $2",
        )
        .bind(next_expiry)
        .bind(security::token_hash(&token))
        .execute(&state.db)
        .await?;
    } else {
        sqlx::query("UPDATE sessions SET last_used_at = now() WHERE token_hash = $1")
            .bind(security::token_hash(&token))
            .execute(&state.db)
            .await?;
    }

    let user = fetch_user(state, session.user_id).await?;
    ensure_email_verified(state, user.email_verified_at)?;
    Ok(AuthenticatedUser { id: user.id })
}

async fn fetch_user(state: &AppState, user_id: Uuid) -> Result<UserRow, AppError> {
    sqlx::query_as::<_, UserRow>(
        "SELECT id, full_name, email, email_verified_at, username FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized {
        code: "AUTHENTICATION_REQUIRED",
        message: "Authentication is required.",
    })
}

async fn fetch_workspace(
    state: &AppState,
    user_id: Uuid,
) -> Result<Option<WorkspaceResponse>, AppError> {
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT w.id, w.name FROM workspaces w INNER JOIN workspace_memberships wm ON wm.workspace_id = w.id WHERE wm.user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(workspace.map(|workspace| WorkspaceResponse {
        id: workspace.id,
        name: workspace.name,
    }))
}

pub(crate) async fn insert_session(
    transaction: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    state: &AppState,
) -> Result<String, AppError> {
    let token = security::random_token();
    let expires_at = OffsetDateTime::now_utc() + Duration::days(state.config.session_ttl_days);
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(security::token_hash(&token))
        .bind(user_id)
        .bind(expires_at)
        .execute(&mut **transaction)
        .await?;
    Ok(token)
}

fn user_response(user: &UserRow) -> UserResponse {
    UserResponse {
        id: user.id,
        full_name: user.full_name.clone(),
        email: user.email.clone(),
        email_verified: user.email_verified_at.is_some(),
        username: user.username.clone(),
    }
}

pub(crate) fn validate_email(email: &str) -> Result<String, &'static str> {
    let normalized = email.trim().to_lowercase();
    let Some((local, domain)) = normalized.split_once('@') else {
        return Err("Enter a valid email address.");
    };
    if local.is_empty()
        || domain.is_empty()
        || !domain.contains('.')
        || normalized.chars().count() > 254
        || normalized.chars().any(char::is_whitespace)
    {
        return Err("Enter a valid email address.");
    }
    Ok(normalized)
}

fn ensure_email_verified(
    state: &AppState,
    verified_at: Option<OffsetDateTime>,
) -> Result<(), AppError> {
    if state.config.auth_require_email_verification && verified_at.is_none() {
        return Err(AppError::Forbidden {
            code: "EMAIL_NOT_VERIFIED",
            message: "Please verify your email before continuing.",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_email() {
        assert_eq!(
            validate_email("  USER@Example.COM ").unwrap(),
            "user@example.com"
        );
    }

    #[tokio::test]
    async fn csrf_reuses_the_browsers_current_token() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let token_from = |response: Response| {
            let cookie = response.headers()[axum::http::header::SET_COOKIE]
                .to_str()
                .unwrap()
                .to_owned();
            cookie.split(';').next().unwrap().split_once('=').unwrap().1.to_owned()
        };
        let first = token_from(csrf(State(state.clone()), HeaderMap::new()).await);
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("{}={first}", state.config.csrf_cookie_name()).parse().unwrap(),
        );
        assert_eq!(token_from(csrf(State(state.clone()), headers).await), first);
        // A malformed cookie is replaced, not echoed back.
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            format!("{}=bad;token", state.config.csrf_cookie_name()).parse().unwrap(),
        );
        assert_ne!(token_from(csrf(State(state), headers).await), "bad");
    }
}
