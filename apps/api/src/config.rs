use std::{env, net::SocketAddr};

use anyhow::{Context, Result, bail};
use axum::http::{HeaderName, HeaderValue, Method, header};
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
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_owned());
        let database_url = env::var("DATABASE_URL").context("DATABASE_URL is required")?;
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
