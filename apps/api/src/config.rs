use std::{
    env,
    net::{IpAddr, SocketAddr},
};

use anyhow::{Context, Result, bail};
use axum::http::{HeaderName, HeaderValue, Method, header};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use sqlx::postgres::PgConnectOptions;
use tower_http::cors::{AllowOrigin, CorsLayer};

const MAX_RESOURCE_STORAGE_BYTES: u64 = 10 * 1024 * 1024 * 1024;
const DEFAULT_APP_SERVICE_PUBLIC_DOMAIN: &str = "knotree.org";

#[derive(Clone, Debug)]
pub struct Config {
    pub sso: Option<crate::sso::SsoConfig>,
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
    pub database_resource_public_host: Option<String>,
    pub database_resource_public_port: Option<u16>,
    pub database_cluster_provider: String,
    pub database_cluster_image: String,
    pub database_cluster_docker_binary: String,
    pub database_cluster_bind_address: IpAddr,
    pub database_cluster_namespace: String,
    pub database_cluster_service_type: String,
    pub database_cluster_storage_size: String,
    pub database_cluster_startup_timeout_seconds: u32,
    pub database_query_timeout_ms: u32,
    pub database_query_max_rows: u32,
    pub app_service_provisioning_enabled: bool,
    pub app_service_public_host: String,
    pub app_service_public_domain: Option<String>,
    pub app_service_public_scheme: String,
    pub app_service_bind_address: IpAddr,
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub github_oauth_redirect_uri: String,
    pub database_credentials_encryption_key: [u8; 32],
    pub kong_admin_url: Option<String>,
    pub kong_traffic_log_endpoint: Option<String>,
    pub kong_traffic_log_token: Option<String>,
    pub docs_dir: String,
    pub mcp_public_base_url: String,
    pub mcp_access_ttl_seconds: i64,
    pub mcp_refresh_ttl_seconds: i64,
    pub redis_cluster_image: String,
    pub app_service_image_pull_secret: Option<String>,
    pub knotree_registry_webhook_secret: Option<String>,
    pub default_rate_limit_rpm: u32,
    pub html_site_data_dir: String,
    pub html_nginx_image: String,
    pub image_public_base_url: String,
    pub image_url_signing_key: [u8; 32],
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
        let database_resource_public_host = optional_host("DATABASE_RESOURCE_PUBLIC_HOST")?;
        let database_resource_public_port = match database_resource_public_host.as_ref() {
            Some(_) => Some(env_u16(
                "DATABASE_RESOURCE_PUBLIC_PORT",
                database_resource_port,
            )?),
            None => optional_u16("DATABASE_RESOURCE_PUBLIC_PORT")?,
        };
        let database_cluster_provider = env::var("DATABASE_CLUSTER_PROVIDER")
            .unwrap_or_else(|_| {
                if app_env == "production" {
                    "kubernetes".to_owned()
                } else {
                    "docker".to_owned()
                }
            })
            .to_ascii_lowercase();
        if !matches!(database_cluster_provider.as_str(), "docker" | "kubernetes") {
            bail!("DATABASE_CLUSTER_PROVIDER must be docker or kubernetes");
        }
        let database_cluster_image =
            env::var("DATABASE_CLUSTER_IMAGE").unwrap_or_else(|_| "postgres:16-alpine".to_owned());
        validate_non_empty_token("DATABASE_CLUSTER_IMAGE", &database_cluster_image)?;
        let database_cluster_docker_binary =
            env::var("DATABASE_CLUSTER_DOCKER_BINARY").unwrap_or_else(|_| "docker".to_owned());
        validate_non_empty_token(
            "DATABASE_CLUSTER_DOCKER_BINARY",
            &database_cluster_docker_binary,
        )?;
        let database_cluster_bind_address = env::var("DATABASE_CLUSTER_BIND_ADDRESS")
            .unwrap_or_else(|_| "127.0.0.1".to_owned())
            .parse::<IpAddr>()
            .context("DATABASE_CLUSTER_BIND_ADDRESS must be a valid IP address")?;
        let database_cluster_namespace = env::var("DATABASE_CLUSTER_NAMESPACE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .or_else(|| env::var("POD_NAMESPACE").ok())
            .unwrap_or_else(|| "default".to_owned());
        validate_kubernetes_name("DATABASE_CLUSTER_NAMESPACE", &database_cluster_namespace)?;
        let database_cluster_service_type =
            env::var("DATABASE_CLUSTER_SERVICE_TYPE").unwrap_or_else(|_| "ClusterIP".to_owned());
        if !matches!(
            database_cluster_service_type.as_str(),
            "ClusterIP" | "LoadBalancer" | "NodePort"
        ) {
            bail!("DATABASE_CLUSTER_SERVICE_TYPE must be ClusterIP, LoadBalancer, or NodePort");
        }
        let database_cluster_storage_size =
            env::var("DATABASE_CLUSTER_STORAGE_SIZE").unwrap_or_else(|_| "10Gi".to_owned());
        validate_storage_size(&database_cluster_storage_size)?;
        let database_cluster_startup_timeout_seconds =
            env_u32("DATABASE_CLUSTER_STARTUP_TIMEOUT_SECONDS", 90)?;
        let database_query_timeout_ms = env_u32("DATABASE_QUERY_TIMEOUT_MS", 10_000)?;
        let database_query_max_rows = env_u32("DATABASE_QUERY_MAX_ROWS", 500)?;
        let app_service_provisioning_enabled =
            env_bool("APP_SERVICE_PROVISIONING_ENABLED", app_env != "production")?;
        let app_service_public_host = env::var("APP_SERVICE_PUBLIC_HOST")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "localhost".to_owned());
        validate_public_host("APP_SERVICE_PUBLIC_HOST", &app_service_public_host)?;
        let app_service_public_domain_env = env::var("APP_SERVICE_PUBLIC_DOMAIN").ok();
        let app_service_public_domain =
            public_domain_setting(app_service_public_domain_env.as_deref())?;
        let app_service_public_scheme = optional_env("APP_SERVICE_PUBLIC_SCHEME")
            .unwrap_or_else(|| {
                if app_service_public_domain.is_some() {
                    "https".to_owned()
                } else {
                    "http".to_owned()
                }
            })
            .to_ascii_lowercase();
        validate_public_scheme("APP_SERVICE_PUBLIC_SCHEME", &app_service_public_scheme)?;
        let app_service_bind_address = env::var("APP_SERVICE_BIND_ADDRESS")
            .unwrap_or_else(|_| "127.0.0.1".to_owned())
            .parse::<IpAddr>()
            .context("APP_SERVICE_BIND_ADDRESS must be a valid IP address")?;
        let github_client_id = optional_env("GITHUB_CLIENT_ID");
        let github_client_secret = optional_env("GITHUB_CLIENT_SECRET");
        if github_client_id.is_some() != github_client_secret.is_some() {
            bail!("GITHUB_CLIENT_ID and GITHUB_CLIENT_SECRET must be configured together");
        }
        let github_oauth_redirect_uri =
            env::var("GITHUB_OAUTH_REDIRECT_URI").unwrap_or_else(|_| {
                if app_env == "production" {
                    "https://cloudapi.knotree.com/api/v1/auth/github/callback".to_owned()
                } else {
                    "http://localhost:8080/api/v1/auth/github/callback".to_owned()
                }
            });
        validate_non_empty_token("GITHUB_OAUTH_REDIRECT_URI", &github_oauth_redirect_uri)?;
        let kong_admin_url = optional_env("KONG_ADMIN_URL");
        let kong_traffic_log_endpoint = optional_env("KONG_TRAFFIC_LOG_ENDPOINT");
        let kong_traffic_log_token = optional_env("KONG_TRAFFIC_LOG_TOKEN");
        let docs_dir = env::var("KNOTREE_DOCS_DIR").unwrap_or_else(|_| {
            if std::path::Path::new("/usr/share/knotree/docs").exists() {
                "/usr/share/knotree/docs".to_owned()
            } else {
                "docs".to_owned()
            }
        });
        let mcp_public_base_url = env::var("MCP_PUBLIC_BASE_URL").unwrap_or_else(|_| {
            if app_env == "production" {
                "https://cloud.knotree.com".to_owned()
            } else {
                "http://localhost:8080".to_owned()
            }
        });
        validate_non_empty_token("MCP_PUBLIC_BASE_URL", &mcp_public_base_url)?;
        let redis_cluster_image =
            env::var("REDIS_CLUSTER_IMAGE").unwrap_or_else(|_| "redis:7-alpine".to_owned());
        validate_non_empty_token("REDIS_CLUSTER_IMAGE", &redis_cluster_image)?;
        let app_service_image_pull_secret = optional_env("APP_SERVICE_IMAGE_PULL_SECRET");
        let knotree_registry_webhook_secret = optional_env("KNOTREE_REGISTRY_WEBHOOK_SECRET");
        if knotree_registry_webhook_secret
            .as_ref()
            .is_some_and(|secret| secret.len() < 32)
        {
            bail!("KNOTREE_REGISTRY_WEBHOOK_SECRET must be at least 32 bytes");
        }
        let default_rate_limit_rpm = env_u32("APP_SERVICE_DEFAULT_RATE_LIMIT_RPM", 60)?;
        let html_site_data_dir = env::var("HTML_SITE_DATA_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "/var/lib/knotree/html-sites".to_owned());
        let html_nginx_image = env::var("HTML_NGINX_IMAGE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| crate::html_pages::HTML_NGINX_IMAGE.to_owned());
        let database_credentials_encryption_key = credentials_encryption_key(&app_env)?;
        let image_url_signing_key = image_signing_key(&database_credentials_encryption_key)?;
        let image_public_base_url = image_public_base_url(&app_env)?;

        Ok(Self {
            sso: crate::sso::SsoConfig::from_env(&app_env)?,
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
            database_resource_public_host,
            database_resource_public_port,
            database_cluster_provider,
            database_cluster_image,
            database_cluster_docker_binary,
            database_cluster_bind_address,
            database_cluster_namespace,
            database_cluster_service_type,
            database_cluster_storage_size,
            database_cluster_startup_timeout_seconds,
            database_query_timeout_ms,
            database_query_max_rows,
            app_service_provisioning_enabled,
            app_service_public_host,
            app_service_public_domain,
            app_service_public_scheme,
            app_service_bind_address,
            github_client_id,
            github_client_secret,
            github_oauth_redirect_uri,
            database_credentials_encryption_key,
            kong_admin_url,
            kong_traffic_log_endpoint,
            kong_traffic_log_token,
            docs_dir,
            mcp_public_base_url,
            mcp_access_ttl_seconds: env_i64("MCP_ACCESS_TTL_SECONDS", 3600)?,
            mcp_refresh_ttl_seconds: env_i64("MCP_REFRESH_TTL_SECONDS", 2_592_000)?,
            redis_cluster_image,
            app_service_image_pull_secret,
            knotree_registry_webhook_secret,
            default_rate_limit_rpm,
            html_site_data_dir,
            html_nginx_image,
            image_public_base_url,
            image_url_signing_key,
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
            .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::OPTIONS])
            .allow_headers([
                header::ACCEPT,
                header::AUTHORIZATION,
                header::CONTENT_TYPE,
                HeaderName::from_static("x-csrf-token"),
            ])
    }
}

pub fn cors_origin_allowed(
    origin: &str,
    allowed_origins: &[String],
    public_scheme: &str,
    public_domain: Option<&str>,
) -> bool {
    if allowed_origins.iter().any(|allowed| allowed == origin) {
        return true;
    }
    is_html_page_origin(origin, public_scheme, public_domain)
}

fn is_html_page_origin(origin: &str, scheme: &str, domain: Option<&str>) -> bool {
    let Some(domain) = domain.filter(|value| !value.is_empty()) else {
        return false;
    };
    let prefix = format!("{scheme}://page-");
    let Some(host) = origin.strip_prefix(&prefix) else {
        return false;
    };
    if host.contains('/') || host.contains(':') || host.contains('@') {
        return false;
    }
    host.ends_with(&format!(".{domain}")) && host.len() > domain.len() + 1
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn public_domain_setting(value: Option<&str>) -> Result<Option<String>> {
    match value {
        None => validate_public_domain(
            "APP_SERVICE_PUBLIC_DOMAIN",
            DEFAULT_APP_SERVICE_PUBLIC_DOMAIN,
        )
        .map(Some),
        Some(value) if value.trim().is_empty() => Ok(None),
        Some(value) => validate_public_domain("APP_SERVICE_PUBLIC_DOMAIN", value).map(Some),
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

fn optional_u16(key: &str) -> Result<Option<u16>> {
    match env::var(key) {
        Ok(value) if !value.trim().is_empty() => {
            let parsed = value
                .parse()
                .with_context(|| format!("{key} must be an integer"))?;
            if parsed == 0 {
                bail!("{key} must be greater than zero");
            }
            Ok(Some(parsed))
        }
        _ => Ok(None),
    }
}

fn optional_host(key: &str) -> Result<Option<String>> {
    match env::var(key).ok().filter(|value| !value.trim().is_empty()) {
        Some(value) => {
            validate_resource_host(&value)?;
            Ok(Some(value))
        }
        None => Ok(None),
    }
}

fn image_public_base_url(app_env: &str) -> Result<String> {
    let value = optional_env("IMAGE_PUBLIC_BASE_URL").unwrap_or_else(|| {
        if app_env == "production" {
            "https://img.knotree.org".to_owned()
        } else {
            "http://localhost:8080".to_owned()
        }
    });
    let trimmed = value.trim_end_matches('/').to_owned();
    if !(trimmed.starts_with("https://") || trimmed.starts_with("http://"))
        || trimmed.contains(' ')
        || trimmed.matches("://").count() != 1
    {
        bail!("IMAGE_PUBLIC_BASE_URL must be an http or https origin");
    }
    Ok(trimmed)
}

fn image_signing_key(encryption_key: &[u8; 32]) -> Result<[u8; 32]> {
    let Some(value) = optional_env("IMAGE_URL_SIGNING_KEY") else {
        // Public URLs stay valid across restarts without a second production
        // secret. Rotating the credentials key also rotates these signatures.
        let mut mac = Hmac::<Sha256>::new_from_slice(encryption_key)
            .expect("HMAC accepts the 32-byte credentials key");
        mac.update(b"knotree-image-url-v1");
        let digest = mac.finalize().into_bytes();
        let mut key = [0_u8; 32];
        key.copy_from_slice(&digest);
        return Ok(key);
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(value.as_bytes())
        .context("IMAGE_URL_SIGNING_KEY must be base64url")?;
    if decoded.len() != 32 {
        bail!("IMAGE_URL_SIGNING_KEY must decode to 32 bytes");
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
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

fn validate_public_host(key: &str, host: &str) -> Result<()> {
    if host.is_empty()
        || host.chars().any(char::is_whitespace)
        || host.contains('/')
        || host.contains(':')
    {
        bail!("{key} must be a hostname or IP address without a scheme or port");
    }
    Ok(())
}

fn validate_public_domain(key: &str, value: &str) -> Result<String> {
    let domain = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if domain.is_empty()
        || domain.len() > 253
        || domain.starts_with('.')
        || domain.contains("..")
        || domain.contains("://")
        || domain.contains('/')
        || domain.contains(':')
        || domain.contains('*')
        || domain.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label.chars().all(|character| {
                    character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
                })
        })
    {
        bail!("{key} must be a DNS domain without a scheme, port, path, or wildcard");
    }
    Ok(domain)
}

fn validate_public_scheme(key: &str, scheme: &str) -> Result<()> {
    if !matches!(scheme, "http" | "https") {
        bail!("{key} must be http or https");
    }
    Ok(())
}

fn validate_non_empty_token(key: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        bail!("{key} must be non-empty and contain no control characters");
    }
    Ok(())
}

fn validate_kubernetes_name(key: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 63
        || value.starts_with('-')
        || value.ends_with('-')
        || !value.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    {
        bail!("{key} must be a lowercase Kubernetes name");
    }
    Ok(())
}

impl Config {
    pub fn uses_kubernetes_workloads(&self) -> bool {
        self.database_cluster_provider == "kubernetes"
    }
}

#[cfg(test)]
impl Config {
    pub fn test_fixture() -> Self {
        Self {
            sso: None,
            database_url: "postgres://postgres:postgres@localhost:5432/knotree_cloud".to_owned(),
            bind_addr: "127.0.0.1:8080".parse().unwrap(),
            app_env: "test".to_owned(),
            allowed_origins: vec!["http://localhost:5173".to_owned()],
            cookie_secure: false,
            auth_require_email_verification: false,
            session_ttl_days: 30,
            database_max_connections: 10,
            database_provisioning_enabled: true,
            database_resource_host: "localhost".to_owned(),
            database_resource_port: 5432,
            database_resource_public_host: None,
            database_resource_public_port: None,
            database_cluster_provider: "kubernetes".to_owned(),
            database_cluster_image: "postgres:16-alpine".to_owned(),
            database_cluster_docker_binary: "docker".to_owned(),
            database_cluster_bind_address: "127.0.0.1".parse().unwrap(),
            database_cluster_namespace: "knotree-clusters".to_owned(),
            database_cluster_service_type: "ClusterIP".to_owned(),
            database_cluster_storage_size: "10Gi".to_owned(),
            database_cluster_startup_timeout_seconds: 90,
            database_query_timeout_ms: 10_000,
            database_query_max_rows: 500,
            app_service_provisioning_enabled: true,
            app_service_public_host: "localhost".to_owned(),
            app_service_public_domain: Some("knotree.org".to_owned()),
            app_service_public_scheme: "https".to_owned(),
            app_service_bind_address: "127.0.0.1".parse().unwrap(),
            github_client_id: None,
            github_client_secret: None,
            github_oauth_redirect_uri: "http://localhost:8080/api/v1/auth/github/callback"
                .to_owned(),
            database_credentials_encryption_key: [7; 32],
            kong_admin_url: None,
            kong_traffic_log_endpoint: None,
            kong_traffic_log_token: None,
            docs_dir: "docs".to_owned(),
            mcp_public_base_url: "http://localhost:8080".to_owned(),
            mcp_access_ttl_seconds: 3600,
            mcp_refresh_ttl_seconds: 2_592_000,
            redis_cluster_image: "redis:7-alpine".to_owned(),
            app_service_image_pull_secret: None,
            knotree_registry_webhook_secret: None,
            default_rate_limit_rpm: 60,
            html_site_data_dir: "/tmp/knotree-html-sites".to_owned(),
            html_nginx_image: "nginxinc/nginx-unprivileged:1.27-alpine".to_owned(),
            image_public_base_url: "http://localhost:8080".to_owned(),
            image_url_signing_key: [9; 32],
        }
    }
}

fn validate_storage_size(value: &str) -> Result<()> {
    let (number, multiplier) = [
        ("Mi", 1024_u64 * 1024),
        ("Gi", 1024_u64 * 1024 * 1024),
        ("Ti", 1024_u64 * 1024 * 1024 * 1024),
    ]
    .iter()
    .find_map(|(suffix, multiplier)| {
        let number = value.strip_suffix(suffix)?.parse::<u64>().ok()?;
        Some((number, *multiplier))
    })
    .context("DATABASE_CLUSTER_STORAGE_SIZE must look like 10Gi, 512Mi, or 1Ti")?;
    let bytes = number
        .checked_mul(multiplier)
        .context("DATABASE_CLUSTER_STORAGE_SIZE is too large")?;
    if number == 0 {
        bail!("DATABASE_CLUSTER_STORAGE_SIZE must be greater than zero");
    }
    if bytes > MAX_RESOURCE_STORAGE_BYTES {
        bail!("DATABASE_CLUSTER_STORAGE_SIZE cannot exceed 10Gi");
    }
    if value.chars().any(char::is_whitespace) {
        bail!("DATABASE_CLUSTER_STORAGE_SIZE must look like 10Gi, 512Mi, or 1Ti");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        cors_origin_allowed, public_domain_setting, validate_public_domain, validate_public_scheme,
        validate_storage_size,
    };

    #[test]
    fn html_page_origins_are_allowed_for_injected_analytics_cors() {
        let dashboard = vec!["https://cloud.knotree.com".to_owned()];
        assert!(cors_origin_allowed(
            "https://cloud.knotree.com",
            &dashboard,
            "https",
            Some("knotree.org"),
        ));
        assert!(cors_origin_allowed(
            "https://page-docs.knotree.org",
            &dashboard,
            "https",
            Some("knotree.org"),
        ));
        assert!(!cors_origin_allowed(
            "https://evil.example",
            &dashboard,
            "https",
            Some("knotree.org"),
        ));
        assert!(!cors_origin_allowed(
            "https://page-.knotree.org",
            &dashboard,
            "https",
            Some("knotree.org"),
        ));
    }

    #[test]
    fn rejects_storage_sizes_above_the_resource_cap() {
        assert!(validate_storage_size("10Gi").is_ok());
        assert!(validate_storage_size("512Mi").is_ok());
        assert!(validate_storage_size("11Gi").is_err());
        assert!(validate_storage_size("0Gi").is_err());
    }

    #[test]
    fn normalizes_and_validates_public_domains() {
        assert_eq!(
            validate_public_domain("APP_SERVICE_PUBLIC_DOMAIN", " Knotree.Org. ").unwrap(),
            "knotree.org"
        );
        assert!(validate_public_domain("APP_SERVICE_PUBLIC_DOMAIN", "*.knotree.org").is_err());
        assert!(
            validate_public_domain("APP_SERVICE_PUBLIC_DOMAIN", "https://knotree.org").is_err()
        );
        assert!(validate_public_domain("APP_SERVICE_PUBLIC_DOMAIN", "knotree..org").is_err());
    }

    #[test]
    fn defaults_public_domains_but_allows_an_explicit_opt_out() {
        assert_eq!(
            public_domain_setting(None).unwrap().as_deref(),
            Some("knotree.org")
        );
        assert_eq!(public_domain_setting(Some(" ")).unwrap(), None);
        assert_eq!(
            public_domain_setting(Some("Example.COM"))
                .unwrap()
                .as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn accepts_only_http_public_url_schemes() {
        assert!(validate_public_scheme("APP_SERVICE_PUBLIC_SCHEME", "http").is_ok());
        assert!(validate_public_scheme("APP_SERVICE_PUBLIC_SCHEME", "https").is_ok());
        assert!(validate_public_scheme("APP_SERVICE_PUBLIC_SCHEME", "ftp").is_err());
    }
}
