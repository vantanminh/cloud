use std::time::Duration;

use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
    response::{IntoResponse, Redirect, Response},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::{Postgres, Transaction};
use url::Url;
use uuid::Uuid;

use crate::{auth, error::AppError, security, state::AppState};

#[derive(Clone, Debug)]
pub struct SsoConfig {
    pub issuer: String,
    pub service_origin: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub frontend_url: String,
}

impl SsoConfig {
    pub fn from_env(app_env: &str) -> anyhow::Result<Option<Self>> {
        let enabled = std::env::var("SSO_ENABLED").unwrap_or_else(|_| "false".into());
        if enabled == "false" {
            return Ok(None);
        }
        anyhow::ensure!(enabled == "true", "SSO_ENABLED must be true or false");
        let issuer = std::env::var("SSO_ISSUER")
            .unwrap_or_else(|_| "https://accounts.knotree.com".into());
        let config = Self {
            service_origin: accounts_service_origin(&issuer)?,
            issuer,
            client_id: std::env::var("SSO_CLIENT_ID").unwrap_or_else(|_| "knotree-cloud".into()),
            redirect_uri: std::env::var("SSO_REDIRECT_URI")
                .unwrap_or_else(|_| "https://cloud.knotree.com/api/v1/auth/sso/callback".into()),
            frontend_url: std::env::var("SSO_FRONTEND_URL")
                .unwrap_or_else(|_| "https://cloud.knotree.com".into()),
        };
        config.validate(app_env == "production")?;
        Ok(Some(config))
    }

    fn validate(&self, production: bool) -> anyhow::Result<()> {
        for value in [&self.issuer, &self.redirect_uri, &self.frontend_url] {
            let url = Url::parse(value)?;
            let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
            anyhow::ensure!(url.host_str().is_some(), "SSO URL must have a host");
            anyhow::ensure!(
                url.scheme() == "https" || (!production && local && url.scheme() == "http"),
                "SSO URLs require HTTPS (loopback HTTP is allowed outside production)"
            );
            anyhow::ensure!(
                url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "SSO URLs must not contain credentials, query or fragment"
            );
        }
        let issuer = Url::parse(&self.issuer)?;
        anyhow::ensure!(
            issuer.path() == "/" && !self.issuer.ends_with('/'),
            "SSO_ISSUER must be an origin without trailing slash"
        );
        anyhow::ensure!(
            !self.client_id.is_empty() && self.client_id.len() <= 128,
            "Invalid SSO_CLIENT_ID"
        );
        Ok(())
    }
}

fn accounts_service_origin(issuer: &str) -> anyhow::Result<String> {
    let value = std::env::var("SSO_SERVICE_ORIGIN").unwrap_or_default();
    if value.is_empty() {
        return Ok(issuer.to_string());
    }
    let url = Url::parse(&value)?;
    anyhow::ensure!(
        url.scheme() == "http"
            && url.host_str() == Some("knotree-accounts.knotree-accounts.svc.cluster.local")
            && url.port().unwrap_or(80) == 80
            && matches!(url.path(), "" | "/")
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "SSO_SERVICE_ORIGIN must be the in-cluster Accounts service"
    );
    Ok(value)
}

fn configured(state: &AppState) -> Result<&SsoConfig, AppError> {
    state
        .config
        .sso
        .as_ref()
        .ok_or(AppError::ServiceUnavailable {
            code: "SSO_NOT_CONFIGURED",
            message: "Knotree Accounts sign-in is not configured.",
        })
}

fn invalid_login() -> AppError {
    AppError::Unauthorized {
        code: "SSO_LOGIN_FAILED",
        message: "Sign-in expired or could not be verified. Start sign-in again.",
    }
}

fn browser_cookie(secure: bool) -> &'static str {
    if secure {
        "__Host-knotree-sso"
    } else {
        "knotree-sso"
    }
}

pub async fn configuration(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"enabled": state.config.sso.is_some()}))
}

pub async fn start(State(state): State<AppState>) -> Result<Response, AppError> {
    let config = configured(&state)?;
    let state_token = security::random_token();
    let browser = security::random_token();
    let verifier = security::random_token();
    let challenge = URL_SAFE_NO_PAD.encode(security::token_hash(&verifier));
    sqlx::query("DELETE FROM sso_login_attempts WHERE expires_at <= now()")
        .execute(&state.db)
        .await?;
    sqlx::query("INSERT INTO sso_login_attempts (state_hash, browser_hash, verifier_ciphertext, issuer, client_id, redirect_uri, expires_at) VALUES ($1,$2,$3,$4,$5,$6,now() + interval '10 minutes')")
        .bind(security::token_hash(&state_token))
        .bind(security::token_hash(&browser))
        .bind(security::encrypt_secret(&verifier, &state.config.database_credentials_encryption_key)?)
        .bind(&config.issuer).bind(&config.client_id).bind(&config.redirect_uri)
        .execute(&state.db).await?;
    let mut url =
        Url::parse(&format!("{}/oauth/authorize", config.issuer)).map_err(|_| invalid_login())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &config.client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", "openid profile email")
        .append_pair("state", &state_token)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    let mut response = Redirect::to(url.as_str()).into_response();
    security::append_cookie(
        &mut response,
        security::cookie_header(
            browser_cookie(state.config.cookie_secure),
            &browser,
            600,
            true,
            state.config.cookie_secure,
        ),
    );
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    Ok(response)
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub state: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
}

pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, AppError> {
    let config = configured(&state)?;
    let state_token = query
        .state
        .filter(|v| v.len() == 43)
        .ok_or_else(invalid_login)?;
    let browser = security::get_cookie(&headers, browser_cookie(state.config.cookie_secure))
        .filter(|v| v.len() == 43)
        .ok_or_else(invalid_login)?;
    // Atomic consumption binds state to this browser and configuration; concurrent
    // callbacks, replays and callbacks after a configuration change fail closed.
    let ciphertext: Option<String> = sqlx::query_scalar("DELETE FROM sso_login_attempts WHERE state_hash=$1 AND browser_hash=$2 AND issuer=$3 AND client_id=$4 AND redirect_uri=$5 AND expires_at > now() RETURNING verifier_ciphertext")
        .bind(security::token_hash(&state_token)).bind(security::token_hash(&browser))
        .bind(&config.issuer).bind(&config.client_id).bind(&config.redirect_uri)
        .fetch_optional(&state.db).await?;
    let ciphertext = ciphertext.ok_or_else(invalid_login)?;
    if query.error.is_some() {
        return Err(invalid_login());
    }
    let code = query
        .code
        .filter(|v| !v.is_empty() && v.len() <= 2048)
        .ok_or_else(invalid_login)?;
    let verifier = security::decrypt_secret(
        &ciphertext,
        &state.config.database_credentials_encryption_key,
    )?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| invalid_login())?;
    let tokens = client
        .post(format!("{}/oauth/token", config.service_origin))
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", &config.client_id),
            ("redirect_uri", &config.redirect_uri),
            ("code", &code),
            ("code_verifier", &verifier),
        ])
        .send()
        .await
        .map_err(|_| invalid_login())?;
    let tokens: TokenResponse = bounded_json(tokens).await?;
    if !tokens.token_type.eq_ignore_ascii_case("bearer") || tokens.access_token.is_empty() {
        return Err(invalid_login());
    }
    // Identity comes from the pinned Accounts userinfo endpoint, which checks
    // the live access grant. Unvalidated ID-token claims are never trusted.
    let profile = client
        .get(format!("{}/oauth/userinfo", config.service_origin))
        .bearer_auth(&tokens.access_token)
        .send()
        .await
        .map_err(|_| invalid_login())?;
    let profile: Profile = bounded_json(profile).await?;
    let mut transaction = state.db.begin().await?;
    let user_id = provision_identity(&mut transaction, &config.issuer, &profile).await?;
    let session = auth::insert_session(&mut transaction, user_id, &state).await?;
    transaction.commit().await?;
    let mut response = Redirect::to(&config.frontend_url).into_response();
    security::append_cookie(
        &mut response,
        security::cookie_header(
            state.config.session_cookie_name(),
            &session,
            state.config.session_ttl_days * 86_400,
            true,
            state.config.cookie_secure,
        ),
    );
    security::append_cookie(
        &mut response,
        security::cookie_header(
            browser_cookie(state.config.cookie_secure),
            "",
            0,
            true,
            state.config.cookie_secure,
        ),
    );
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("referrer-policy", "no-referrer".parse().unwrap());
    Ok(response)
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
}
#[derive(Deserialize, Serialize)]
struct Profile {
    sub: String,
    email: String,
    email_verified: bool,
    name: Option<String>,
}

async fn bounded_json<T: serde::de::DeserializeOwned>(
    mut response: reqwest::Response,
) -> Result<T, AppError> {
    if !response.status().is_success() {
        return Err(invalid_login());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| invalid_login())? {
        if bytes.len() + chunk.len() > 16 * 1024 {
            return Err(invalid_login());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| invalid_login())
}

async fn provision_identity(
    transaction: &mut Transaction<'_, Postgres>,
    issuer: &str,
    profile: &Profile,
) -> Result<Uuid, AppError> {
    if !profile.email_verified
        || profile.sub.is_empty()
        || profile.sub.len() > 255
        || profile.sub.chars().any(char::is_control)
    {
        return Err(invalid_login());
    }
    let email = auth::validate_email(&profile.email).map_err(|_| invalid_login())?;
    let identity_key =
        serde_json::to_string(&(issuer, &profile.sub)).map_err(AppError::internal)?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(identity_key)
        .execute(&mut **transaction)
        .await?;
    if let Some(user_id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM sso_identities WHERE issuer=$1 AND subject=$2",
    )
    .bind(issuer)
    .bind(&profile.sub)
    .fetch_optional(&mut **transaction)
    .await?
    {
        return Ok(user_id);
    }
    let name: String = profile
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Knotree user")
        .chars()
        .take(80)
        .collect();
    let user_id = Uuid::new_v4();
    let inserted = sqlx::query("INSERT INTO users (id, full_name, email, password_hash, email_verified_at) VALUES ($1,$2,$3,'!sso-only',now()) ON CONFLICT DO NOTHING")
        .bind(user_id).bind(name).bind(email).execute(&mut **transaction).await?;
    if inserted.rows_affected() != 1 {
        return Err(AppError::Conflict {
            code: "SSO_ACCOUNT_LINK_REQUIRED",
            message: "A Cloud account already uses this email. Sign in to that account to link your Knotree identity.",
        });
    }
    sqlx::query("INSERT INTO sso_identities (issuer, subject, user_id) VALUES ($1,$2,$3)")
        .bind(issuer)
        .bind(&profile.sub)
        .bind(user_id)
        .execute(&mut **transaction)
        .await?;
    Ok(user_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Form, Router,
        http::header,
        routing::{get, post},
    };
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex},
    };

    #[test]
    fn configuration_rejects_insecure_or_ambiguous_endpoints() {
        let mut config = SsoConfig {
            issuer: "https://accounts.knotree.com".into(),
            service_origin: "https://accounts.knotree.com".into(),
            client_id: "knotree-cloud".into(),
            redirect_uri: "https://cloud.knotree.com/api/v1/auth/sso/callback".into(),
            frontend_url: "https://cloud.knotree.com".into(),
        };
        assert!(config.validate(true).is_ok());
        for issuer in [
            "http://accounts.knotree.com",
            "https://user:pass@accounts.knotree.com",
            "https://accounts.knotree.com/path",
            "https://accounts.knotree.com?issuer=other",
            "https://accounts.knotree.com/",
        ] {
            config.issuer = issuer.into();
            assert!(config.validate(true).is_err(), "{issuer}");
        }
        config.issuer = "http://127.0.0.1:4567".into();
        assert!(config.validate(false).is_ok());
        assert!(config.validate(true).is_err());
    }

    #[tokio::test]
    async fn callback_enforces_pkce_browser_binding_and_single_use_then_creates_session() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let expected_challenge = Arc::new(Mutex::new(String::new()));
        let challenge_for_server = expected_challenge.clone();
        let email = format!("sso-{}@example.com", Uuid::new_v4());
        let profile = serde_json::json!({"sub":Uuid::new_v4().to_string(), "email":email, "email_verified":true, "name":"SSO User"});
        let mock = Router::new()
            .route(
                "/oauth/token",
                post(move |Form(form): Form<HashMap<String, String>>| {
                    let challenge = challenge_for_server.lock().unwrap().clone();
                    async move {
                        assert_eq!(form.get("grant_type").unwrap(), "authorization_code");
                        assert_eq!(form.get("client_id").unwrap(), "knotree-cloud");
                        assert_eq!(
                            form.get("redirect_uri").unwrap(),
                            "http://localhost:8080/api/v1/auth/sso/callback"
                        );
                        assert_eq!(
                            URL_SAFE_NO_PAD
                                .encode(security::token_hash(form.get("code_verifier").unwrap())),
                            challenge
                        );
                        Json(
                            serde_json::json!({"access_token":"live-token", "token_type":"Bearer"}),
                        )
                    }
                }),
            )
            .route(
                "/oauth/userinfo",
                get(move |headers: HeaderMap| {
                    let profile = profile.clone();
                    async move {
                        assert_eq!(
                            headers.get(header::AUTHORIZATION).unwrap(),
                            "Bearer live-token"
                        );
                        Json(profile)
                    }
                }),
            );
        let server = tokio::spawn(async move {
            axum::serve(listener, mock).await.unwrap();
        });
        let Some(state) = crate::test_support::test_app_state_configured(|config| {
            config.sso = Some(SsoConfig {
                issuer: issuer.clone(),
                service_origin: issuer.clone(),
                client_id: "knotree-cloud".into(),
                redirect_uri: "http://localhost:8080/api/v1/auth/sso/callback".into(),
                frontend_url: "http://localhost:5173".into(),
            });
        })
        .await
        else {
            server.abort();
            return;
        };
        let response = start(State(state.clone())).await.unwrap();
        let url = Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let params: HashMap<String, String> = url
            .query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        assert_eq!(params["code_challenge_method"], "S256");
        assert_eq!(params["scope"], "openid profile email");
        *expected_challenge.lock().unwrap() = params["code_challenge"].clone();
        let cookie = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("knotree-sso={}", security::random_token())
                .parse()
                .unwrap(),
        );
        let query = || {
            Query(CallbackQuery {
                state: Some(params["state"].clone()),
                code: Some("code-once".into()),
                error: None,
            })
        };
        assert!(
            callback(State(state.clone()), headers, query())
                .await
                .is_err()
        );
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, cookie.parse().unwrap());
        let result = callback(State(state.clone()), headers.clone(), query())
            .await
            .unwrap();
        assert_eq!(result.headers()[header::LOCATION], "http://localhost:5173");
        let session_cookie = result
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|h| h.to_str().unwrap())
            .find(|h| h.starts_with(state.config.session_cookie_name()))
            .unwrap()
            .split(';')
            .next()
            .unwrap();
        let mut session_headers = HeaderMap::new();
        session_headers.insert(header::COOKIE, session_cookie.parse().unwrap());
        let user = auth::authenticate(&state, &session_headers).await.unwrap();
        assert!(
            callback(State(state.clone()), headers, query())
                .await
                .is_err()
        );
        sqlx::query("DELETE FROM users WHERE id=$1")
            .bind(user.id)
            .execute(&state.db)
            .await
            .unwrap();
        server.abort();
    }

    #[tokio::test]
    async fn identity_mapping_never_links_by_email_and_rejects_unverified_email() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let mut transaction = state.db.begin().await.unwrap();
        let email = format!("sso-{}@example.com", Uuid::new_v4());
        let mut profile = Profile {
            sub: Uuid::new_v4().to_string(),
            email,
            email_verified: false,
            name: Some("SSO User".into()),
        };
        assert!(
            provision_identity(&mut transaction, "https://accounts.knotree.com", &profile)
                .await
                .is_err()
        );
        profile.email_verified = true;
        let user = provision_identity(&mut transaction, "https://accounts.knotree.com", &profile)
            .await
            .unwrap();
        let hash: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(&mut *transaction)
            .await
            .unwrap();
        assert!(!security::verify_password("any-password", &hash));
        let original_sub = profile.sub.clone();
        profile.sub = Uuid::new_v4().to_string();
        assert!(matches!(
            provision_identity(&mut transaction, "https://accounts.knotree.com", &profile).await,
            Err(AppError::Conflict {
                code: "SSO_ACCOUNT_LINK_REQUIRED",
                ..
            })
        ));
        profile.sub = original_sub;
        assert!(
            provision_identity(&mut transaction, "https://other.example.com", &profile)
                .await
                .is_err()
        );
        profile.email = "changed@example.com".into();
        assert_eq!(
            provision_identity(&mut transaction, "https://accounts.knotree.com", &profile)
                .await
                .unwrap(),
            user
        );
        transaction.rollback().await.unwrap();
    }

    #[tokio::test]
    async fn expired_login_attempt_does_not_authenticate() {
        let Some(state) = crate::test_support::test_app_state_configured(|config| {
            config.sso = Some(SsoConfig {
                issuer: "https://accounts.knotree.com".into(),
                service_origin: "https://accounts.knotree.com".into(),
                client_id: "knotree-cloud".into(),
                redirect_uri: "https://cloud.knotree.com/api/v1/auth/sso/callback".into(),
                frontend_url: "https://cloud.knotree.com".into(),
            });
        })
        .await
        else {
            return;
        };
        let response = start(State(state.clone())).await.unwrap();
        let url = Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let state_token = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        sqlx::query("UPDATE sso_login_attempts SET expires_at=now()-interval '1 second' WHERE state_hash=$1").bind(security::token_hash(&state_token)).execute(&state.db).await.unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        let result = callback(
            State(state.clone()),
            headers,
            Query(CallbackQuery {
                state: Some(state_token.clone()),
                code: Some("expired".into()),
                error: None,
            }),
        )
        .await;
        assert!(matches!(result, Err(AppError::Unauthorized { .. })));
        sqlx::query("DELETE FROM sso_login_attempts WHERE state_hash=$1")
            .bind(security::token_hash(&state_token))
            .execute(&state.db)
            .await
            .unwrap();
    }
}
