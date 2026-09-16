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

pub fn declarative_config(routes: &[KongAppRoute]) -> Result<Value> {
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
            "plugins": [kong_rate_limiting_plugin(policy)],
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
) -> Option<KongAppRoute> {
    let public_host = public_host.filter(|host| !host.is_empty())?;
    Some(KongAppRoute {
        service_id,
        public_host,
        upstream_url: upstream_url.to_owned(),
        rate_limit_rpm,
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
            },
            KongAppRoute {
                service_id: second,
                public_host: "app-two.knotree.org".to_owned(),
                upstream_url: "http://knotree-app-two:8080".to_owned(),
                rate_limit_rpm: 200,
            },
        ])
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
    fn disabled_services_are_not_added_to_kong() {
        assert!(
            route_for_enabled_service(
                Uuid::nil(),
                None,
                "http://knotree-app:8080",
                60
            )
            .is_none()
        );
        assert!(
            route_for_enabled_service(
                Uuid::nil(),
                Some(String::new()),
                "http://knotree-app:8080",
                60
            )
            .is_none()
        );
        assert!(
            route_for_enabled_service(
                Uuid::nil(),
                Some("app-ready.knotree.org".to_owned()),
                "http://knotree-app:8080",
                90
            )
            .is_some()
        );
    }
}
