use std::{env, net::SocketAddr};

use anyhow::{Context, Result, bail};
use axum::http::{HeaderName, HeaderValue, Method, header};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgConnectOptions;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub bind_addr: SocketAddr,
    pub app_env: String,
    pub allowed_origins: Vec<String>,
    pub cookie_secure: bool,
    pub auth_require_email_verification: bool,
    pub session_ttl_days: i64,
    pub database_max_connections: u32,
    pub database_provisioning_enabled: bool,
    pub database_resource_host: String,
    pub database_resource_port: u16,
    pub database_credentials_encryption_key: [u8; 32],
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned());
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
        let database_options = database_url
            .parse::<PgConnectOptions>()
            .context("DATABASE_URL must be a valid PostgreSQL URL")?;
        let bind_addr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8080".to_owned())
            .parse()
            .context("BIND_ADDR must be a valid socket address")?;
        let default_origin = if app_env == "production" {
            "https://cloud.knotree.com"
        } else {
            "http://localhost:5173"
        };
        let allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| default_origin.to_owned())
            .split(',')
            .map(str::trim)
            .filter(|origin| !origin.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if allowed_origins.is_empty() {
            bail!("CORS_ALLOWED_ORIGINS must contain at least one origin");
        }

        let database_resource_host = env::var("DATABASE_RESOURCE_HOST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| database_options.get_host().to_owned());
        validate_resource_host(&database_resource_host)?;
        let database_resource_port =
            env_u16("DATABASE_RESOURCE_PORT", database_options.get_port())?;

        Ok(Self {
            database_url,
            bind_addr,
            cookie_secure: env_bool("COOKIE_SECURE", app_env == "production")?,
            auth_require_email_verification: env_bool(
                "AUTH_REQUIRE_EMAIL_VERIFICATION",
                app_env == "production",
            )?,
            session_ttl_days: env_i64("SESSION_TTL_DAYS", 30)?,
            database_max_connections: env_u32("DATABASE_MAX_CONNECTIONS", 10)?,
            database_provisioning_enabled: env_bool(
                "DATABASE_PROVISIONING_ENABLED",
                app_env != "production",
            )?,
            database_resource_host,
            database_resource_port,
            database_credentials_encryption_key: credentials_encryption_key(&app_env)?,
            app_env,
            allowed_origins,
        })
    }

    pub fn session_cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-knotree_session"
        } else {
            "knotree_session"
        }
    }

    pub fn csrf_cookie_name(&self) -> &'static str {
        if self.cookie_secure {
            "__Host-knotree_csrf"
        } else {
            "knotree_csrf"
        }
    }

    pub fn is_allowed_origin(&self, origin: &str) -> bool {
        self.allowed_origins.iter().any(|allowed| allowed == origin)
    }

    pub fn cors_layer(&self) -> CorsLayer {
        let origins = self
            .allowed_origins
            .iter()
            .map(|origin| HeaderValue::from_str(origin).expect("validated CORS origin"))
            .collect::<Vec<_>>();

        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_credentials(true)
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([
                header::ACCEPT,
                header::CONTENT_TYPE,
                HeaderName::from_static("x-csrf-token"),
            ])
    }
}

fn env_bool(key: &str, default: bool) -> Result<bool> {
    match env::var(key) {
        Ok(value) => value
            .parse()
            .with_context(|| format!("{key} must be true or false")),
        Err(_) => Ok(default),
    }
}

fn env_i64(key: &str, default: i64) -> Result<i64> {
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    let parsed = value
        .parse()
        .with_context(|| format!("{key} must be an integer"))?;
    if parsed <= 0 {
        bail!("{key} must be greater than zero");
    }
    Ok(parsed)
}

fn env_u32(key: &str, default: u32) -> Result<u32> {
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    let parsed = value
        .parse()
        .with_context(|| format!("{key} must be an integer"))?;
    if parsed == 0 {
        bail!("{key} must be greater than zero");
    }
    Ok(parsed)
}

fn env_u16(key: &str, default: u16) -> Result<u16> {
    let value = env::var(key).unwrap_or_else(|_| default.to_string());
    let parsed = value
        .parse()
        .with_context(|| format!("{key} must be an integer"))?;
    if parsed == 0 {
        bail!("{key} must be greater than zero");
    }
    Ok(parsed)
}

fn credentials_encryption_key(app_env: &str) -> Result<[u8; 32]> {
    let Some(value) = env::var("DATABASE_CREDENTIALS_ENCRYPTION_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        if app_env == "production" {
            bail!("DATABASE_CREDENTIALS_ENCRYPTION_KEY is required in production");
        }

        // Development-only fallback. Production must provide a stable secret so
        // credentials remain decryptable after an API restart.
        let digest = Sha256::digest(b"knotree-development-credentials-key");
        let mut key = [0_u8; 32];
        key.copy_from_slice(&digest);
        return Ok(key);
    };

    let decoded = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .context("DATABASE_CREDENTIALS_ENCRYPTION_KEY must be base64url")?;
    if decoded.len() != 32 {
        bail!("DATABASE_CREDENTIALS_ENCRYPTION_KEY must decode to 32 bytes");
    }

    let mut key = [0_u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn validate_resource_host(host: &str) -> Result<()> {
    if host.is_empty() || host.chars().any(char::is_whitespace) || host.contains('/') {
        bail!("DATABASE_RESOURCE_HOST must be a hostname or IP address");
    }
    Ok(())
}
