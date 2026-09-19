use anyhow::{Context, Result};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::limits::{RateLimitPolicy, validate_rate_limit_rpm};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KongAppRoute {
    pub service_id: Uuid,
    pub public_host: String,
    pub upstream_url: String,
    pub rate_limit_rpm: u32,
    pub cache_html: bool,
}

pub fn kong_rate_limiting_plugin(policy: RateLimitPolicy) -> Value {
    json!({
        "name": "rate-limiting",
        "config": {
            "minute": policy.requests_per_minute,
            "limit_by": "ip",
            "policy": "local",
            "fault_tolerant": true,
            "hide_client_headers": false,
        },
    })
}

pub fn kong_html_cache_plugin() -> Value {
    json!({
        "name": "proxy-cache",
        "config": {
            "response_code": [200],
            "request_method": ["GET", "HEAD"],
            "content_type": [
                "text/html",
                "text/html; charset=utf-8",
                "text/css",
                "text/css; charset=utf-8",
                "text/javascript",
                "text/javascript; charset=utf-8",
                "application/javascript",
                "application/javascript; charset=utf-8",
                "image/png",
                "image/jpeg",
                "image/gif",
                "image/svg+xml",
                "image/webp",
                "image/x-icon",
                "font/woff2"
            ],
            "cache_ttl": 300,
            "strategy": "memory",
            "cache_control": true,
            "vary_headers": ["accept", "accept-encoding"],
        },
    })
}

fn kong_http_log_plugin(endpoint: &str, token: &str, service_id: Uuid) -> Value {
    json!({
        "name": "http-log",
        "config": {
            "http_endpoint": format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                service_id.simple()
            ),
            "method": "POST",
            "content_type": "application/json",
            "headers": {
                "x-knotree-traffic-token": token,
            },
        },
    })
}

pub fn declarative_config(
    routes: &[KongAppRoute],
    traffic_log_endpoint: Option<&str>,
    traffic_log_token: Option<&str>,
) -> Result<Value> {
    let mut services = Vec::with_capacity(routes.len());
    for route in routes {
        let policy = validate_rate_limit_rpm(route.rate_limit_rpm)
            .map_err(anyhow::Error::msg)
            .with_context(|| {
                format!(
                    "invalid Kong rate limit for app service {}",
                    route.service_id
                )
            })?;
        let name = format!("knotree-app-{}", route.service_id.simple());
        let mut plugins = vec![kong_rate_limiting_plugin(policy)];
        if route.cache_html {
            plugins.push(kong_html_cache_plugin());
        }
        if let (Some(endpoint), Some(token)) = (traffic_log_endpoint, traffic_log_token) {
            if !endpoint.trim().is_empty() && !token.trim().is_empty() {
                plugins.push(kong_http_log_plugin(endpoint, token, route.service_id));
            }
        }
        services.push(json!({
            "name": name,
            "url": route.upstream_url,
            "connect_timeout": 5_000,
            "read_timeout": 60_000,
            "write_timeout": 60_000,
            "retries": 1,
            "routes": [{
                "name": format!("{name}-public"),
                "hosts": [route.public_host],
                "strip_path": false,
                "preserve_host": true,
            }],
            "plugins": plugins,
        }));
    }
    Ok(json!({
        "_format_version": "3.0",
        "_transform": true,
        "services": services,
    }))
}

pub fn route_for_enabled_service(
    service_id: Uuid,
    public_host: Option<String>,
    upstream_url: &str,
    rate_limit_rpm: u32,
    cache_html: bool,
) -> Option<KongAppRoute> {
    let public_host = public_host.filter(|host| !host.is_empty())?;
    Some(KongAppRoute {
        service_id,
        public_host,
        upstream_url: upstream_url.to_owned(),
        rate_limit_rpm,
        cache_html,
    })
}

pub async fn apply_declarative_config(admin_url: &str, config: &Value) -> Result<()> {
    let url = format!("{}/config", admin_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(url)
        .json(config)
        .send()
        .await
        .context("could not reach the Kong Admin API")?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Kong rejected declarative config ({status}): {body}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::RateLimitPolicy;

    #[test]
    fn kong_plugin_uses_the_custom_per_service_limit() {
        let plugin = kong_rate_limiting_plugin(RateLimitPolicy {
            requests_per_minute: 120,
        });
        assert_eq!(plugin["name"], "rate-limiting");
        assert_eq!(plugin["config"]["minute"], 120);
        assert_eq!(plugin["config"]["limit_by"], "ip");
    }

    #[test]
    fn declarative_config_keeps_services_isolated_with_their_own_limits() {
        let first = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let second = Uuid::parse_str("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap();
        let config = declarative_config(&[
            KongAppRoute {
                service_id: first,
                public_host: "app-one.knotree.org".to_owned(),
                upstream_url: "http://knotree-app-one:8080".to_owned(),
                rate_limit_rpm: 30,
                cache_html: false,
            },
            KongAppRoute {
                service_id: second,
                public_host: "app-two.knotree.org".to_owned(),
                upstream_url: "http://knotree-app-two:8080".to_owned(),
                rate_limit_rpm: 200,
                cache_html: false,
            },
        ], None, None)
        .unwrap();

        let services = config["services"].as_array().unwrap();
        assert_eq!(services.len(), 2);
        assert_eq!(services[0]["plugins"][0]["config"]["minute"], 30);
        assert_eq!(services[1]["plugins"][0]["config"]["minute"], 200);
        assert_eq!(services[0]["routes"][0]["hosts"][0], "app-one.knotree.org");
        assert_ne!(
            services[0]["routes"][0]["hosts"][0],
            services[1]["routes"][0]["hosts"][0]
        );
    }

    #[test]
    fn declarative_config_logs_each_service_request_to_the_api() {
        let service_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let config = declarative_config(
            &[KongAppRoute {
                service_id,
                public_host: "app-one.knotree.org".to_owned(),
                upstream_url: "http://knotree-app-one:8080".to_owned(),
                rate_limit_rpm: 30,
                cache_html: false,
            }],
            Some("http://knotree-api:8080/internal/public-traffic"),
            Some("traffic-secret"),
        )
        .unwrap();

        let plugins = config["services"][0]["plugins"].as_array().unwrap();
        assert_eq!(plugins.len(), 2);
        assert_eq!(plugins[1]["name"], "http-log");
        assert_eq!(
            plugins[1]["config"]["http_endpoint"],
            format!(
                "http://knotree-api:8080/internal/public-traffic/{}",
                service_id.simple()
            )
        );
        assert_eq!(
            plugins[1]["config"]["headers"]["x-knotree-traffic-token"],
            "traffic-secret"
        );
    }

    #[test]
    fn disabled_services_are_not_added_to_kong() {
        assert!(
            route_for_enabled_service(
                Uuid::nil(),
                None,
                "http://knotree-app:8080",
                60,
                false,
            )
            .is_none()
        );
        assert!(
            route_for_enabled_service(
                Uuid::nil(),
                Some(String::new()),
                "http://knotree-app:8080",
                60,
                false,
            )
            .is_none()
        );
        assert!(
            route_for_enabled_service(
                Uuid::nil(),
                Some("app-ready.knotree.org".to_owned()),
                "http://knotree-app:8080",
                90,
                false,
            )
            .is_some()
        );
    }

    #[test]
    fn html_pages_enable_kong_proxy_cache_for_cloudflare_origin_offload() {
        let plugin = kong_html_cache_plugin();
        assert_eq!(plugin["name"], "proxy-cache");
        assert_eq!(plugin["config"]["cache_control"], true);
        assert_eq!(plugin["config"]["strategy"], "memory");
        let config = declarative_config(
            &[KongAppRoute {
                service_id: Uuid::nil(),
                public_host: "page-docs.knotree.org".to_owned(),
                upstream_url: "http://knotree-html:8080".to_owned(),
                rate_limit_rpm: 60,
                cache_html: true,
            }],
            None,
            None,
        )
        .unwrap();
        let plugins = config["services"][0]["plugins"].as_array().unwrap();
        assert_eq!(plugins[1]["name"], "proxy-cache");
    }
}
