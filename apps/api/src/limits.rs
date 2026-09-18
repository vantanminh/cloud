use std::collections::HashMap;

use serde_json::{Value, json};

pub const RESOURCE_CPU_LIMIT: &str = "1";
/// The CPU allocation shown in the product contract is virtual. Kubernetes
/// App services use a smaller host quota so a small node can run many mostly
/// idle services without reserving one physical core per service.
pub const APP_SERVICE_VIRTUAL_CPU: &str = "1 vCPU";
pub const KUBERNETES_APP_CPU_REQUEST: &str = "1m";
pub const KUBERNETES_APP_CPU_LIMIT: &str = "250m";
pub const KUBERNETES_APP_MEMORY_REQUEST: &str = "32Mi";
pub const KUBERNETES_APP_EPHEMERAL_STORAGE_REQUEST: &str = "64Mi";
pub const RESOURCE_MEMORY_LIMIT_DOCKER: &str = "1g";
pub const RESOURCE_MEMORY_SWAP_LIMIT_DOCKER: &str = "1g";
pub const RESOURCE_MEMORY_LIMIT_KUBERNETES: &str = "1Gi";
pub const RESOURCE_MEMORY_LIMIT_BYTES: i64 = 1024 * 1024 * 1024;
pub const RESOURCE_VOLUME_LIMIT_DOCKER: &str = "10G";
pub const RESOURCE_VOLUME_LIMIT_KUBERNETES: &str = "10Gi";
pub const RESOURCE_VOLUME_LIMIT_BYTES: i64 = 10 * 1024 * 1024 * 1024;
pub const RESOURCE_STORAGE_LIMIT_MESSAGE: &str =
    "This resource reached its 10 GiB storage limit and was stopped.";
pub const DEFAULT_APP_RATE_LIMIT_RPM: u32 = 60;
pub const MIN_APP_RATE_LIMIT_RPM: u32 = 1;
pub const MAX_APP_RATE_LIMIT_RPM: u32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceCaps {
    pub cpu: &'static str,
    pub memory_docker: &'static str,
    pub memory_kubernetes: &'static str,
    pub storage_docker: &'static str,
    pub storage_kubernetes: &'static str,
    pub storage_bytes: i64,
}

pub const TENANT_RESOURCE_CAPS: ResourceCaps = ResourceCaps {
    cpu: RESOURCE_CPU_LIMIT,
    memory_docker: RESOURCE_MEMORY_LIMIT_DOCKER,
    memory_kubernetes: RESOURCE_MEMORY_LIMIT_KUBERNETES,
    storage_docker: RESOURCE_VOLUME_LIMIT_DOCKER,
    storage_kubernetes: RESOURCE_VOLUME_LIMIT_KUBERNETES,
    storage_bytes: RESOURCE_VOLUME_LIMIT_BYTES,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitPolicy {
    pub requests_per_minute: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RateLimitDecision {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
}

#[derive(Debug, Default)]
pub struct PerKeyMinuteLimiter {
    windows: HashMap<String, (u64, u32)>,
}

impl PerKeyMinuteLimiter {
    pub fn check(&mut self, key: &str, limit: u32, now_unix: u64) -> RateLimitDecision {
        let limit = limit.clamp(MIN_APP_RATE_LIMIT_RPM, MAX_APP_RATE_LIMIT_RPM);
        let minute = now_unix / 60;
        let entry = self.windows.entry(key.to_owned()).or_insert((minute, 0));
        if entry.0 != minute {
            *entry = (minute, 0);
        }
        if entry.1 >= limit {
            return RateLimitDecision {
                allowed: false,
                limit,
                remaining: 0,
            };
        }
        entry.1 += 1;
        RateLimitDecision {
            allowed: true,
            limit,
            remaining: limit.saturating_sub(entry.1),
        }
    }
}

pub fn validate_rate_limit_rpm(value: u32) -> Result<RateLimitPolicy, &'static str> {
    if (MIN_APP_RATE_LIMIT_RPM..=MAX_APP_RATE_LIMIT_RPM).contains(&value) {
        Ok(RateLimitPolicy {
            requests_per_minute: value,
        })
    } else {
        Err("Rate limit must be between 1 and 10000 requests per minute.")
    }
}

pub fn docker_runtime_limit_args() -> Vec<String> {
    vec![
        "--cpus".to_owned(),
        TENANT_RESOURCE_CAPS.cpu.to_owned(),
        "--memory".to_owned(),
        TENANT_RESOURCE_CAPS.memory_docker.to_owned(),
        "--memory-swap".to_owned(),
        RESOURCE_MEMORY_SWAP_LIMIT_DOCKER.to_owned(),
    ]
}

pub fn docker_resource_limit_args() -> Vec<String> {
    let mut args = docker_runtime_limit_args();
    args.extend([
        "--storage-opt".to_owned(),
        format!("size={}", TENANT_RESOURCE_CAPS.storage_docker),
    ]);
    args
}

pub fn kubernetes_resource_requirements() -> Value {
    json!({
        "requests": {
            "cpu": "100m",
            "memory": "128Mi",
            "ephemeral-storage": "256Mi",
        },
        "limits": {
            "cpu": TENANT_RESOURCE_CAPS.cpu,
            "memory": TENANT_RESOURCE_CAPS.memory_kubernetes,
            "ephemeral-storage": TENANT_RESOURCE_CAPS.storage_kubernetes,
        },
    })
}

/// Requirements for a Kubernetes App service.
///
/// Do not remove requests entirely: when a limit is present and a request is
/// omitted, Kubernetes may copy the limit into the request and recreate the
/// capacity problem this policy is intended to solve. These small requests
/// reserve only startup headroom; the product-facing 1 vCPU allocation is a
/// virtual plan value while the actual host CPU quota is 250m.
pub fn kubernetes_app_resource_requirements() -> Value {
    json!({
        "requests": {
            "cpu": KUBERNETES_APP_CPU_REQUEST,
            "memory": KUBERNETES_APP_MEMORY_REQUEST,
            "ephemeral-storage": KUBERNETES_APP_EPHEMERAL_STORAGE_REQUEST,
        },
        "limits": {
            "cpu": KUBERNETES_APP_CPU_LIMIT,
            "memory": TENANT_RESOURCE_CAPS.memory_kubernetes,
            "ephemeral-storage": TENANT_RESOURCE_CAPS.storage_kubernetes,
        },
    })
}

/// PostgreSQL keeps the same tenant hard limits as every other managed
/// workload, but uses a small scheduler request so a single-node cluster can
/// admit a database while its actual usage is idle. The limit still protects
/// the node from a database consuming more than the product cap.
pub fn kubernetes_database_resource_requirements() -> Value {
    json!({
        "requests": {
            "cpu": "1m",
            "memory": "64Mi",
            "ephemeral-storage": "256Mi",
        },
        "limits": {
            "cpu": TENANT_RESOURCE_CAPS.cpu,
            "memory": TENANT_RESOURCE_CAPS.memory_kubernetes,
            "ephemeral-storage": TENANT_RESOURCE_CAPS.storage_kubernetes,
        },
    })
}

pub fn kubernetes_storage_request() -> Value {
    json!({
        "requests": { "storage": TENANT_RESOURCE_CAPS.storage_kubernetes },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_caps_apply_to_app_postgres_and_redis() {
        assert_eq!(TENANT_RESOURCE_CAPS.cpu, "1");
        assert_eq!(TENANT_RESOURCE_CAPS.memory_docker, "1g");
        assert_eq!(TENANT_RESOURCE_CAPS.memory_kubernetes, "1Gi");
        assert_eq!(TENANT_RESOURCE_CAPS.storage_bytes, 10 * 1024 * 1024 * 1024);
        let docker = docker_resource_limit_args();
        assert!(docker.windows(2).any(|pair| pair == ["--cpus", "1"]));
        assert!(docker.windows(2).any(|pair| pair == ["--memory", "1g"]));
        assert!(
            docker
                .windows(2)
                .any(|pair| pair == ["--storage-opt", "size=10G"])
        );
        let kube = kubernetes_resource_requirements();
        let database_kube = kubernetes_database_resource_requirements();
        assert_eq!(database_kube["requests"]["cpu"], "1m");
        assert_eq!(database_kube["requests"]["memory"], "64Mi");
        assert_eq!(database_kube["limits"]["cpu"], "1");
        assert_eq!(database_kube["limits"]["memory"], "1Gi");
        assert_eq!(kube["limits"]["cpu"], "1");
        assert_eq!(kube["limits"]["memory"], "1Gi");
        assert_eq!(kube["limits"]["ephemeral-storage"], "10Gi");
        assert_eq!(kubernetes_storage_request()["requests"]["storage"], "10Gi");
    }

    #[test]
    fn app_services_use_a_virtual_cpu_allocation_with_small_scheduler_requests() {
        let app = kubernetes_app_resource_requirements();
        assert_eq!(APP_SERVICE_VIRTUAL_CPU, "1 vCPU");
        assert_eq!(app["requests"]["cpu"], KUBERNETES_APP_CPU_REQUEST);
        assert_eq!(app["requests"]["memory"], KUBERNETES_APP_MEMORY_REQUEST);
        assert_eq!(
            app["requests"]["ephemeral-storage"],
            KUBERNETES_APP_EPHEMERAL_STORAGE_REQUEST
        );
        assert_eq!(app["limits"]["cpu"], KUBERNETES_APP_CPU_LIMIT);
        assert_eq!(app["limits"]["memory"], "1Gi");
        assert_eq!(app["limits"]["ephemeral-storage"], "10Gi");
    }

    #[test]
    fn rejects_rate_limits_outside_the_safe_window() {
        assert_eq!(validate_rate_limit_rpm(60).unwrap().requests_per_minute, 60);
        assert!(validate_rate_limit_rpm(0).is_err());
        assert!(validate_rate_limit_rpm(10_001).is_err());
    }

    #[test]
    fn isolates_rate_limits_so_one_service_cannot_starve_another() {
        let mut limiter = PerKeyMinuteLimiter::default();
        for _ in 0..60 {
            assert!(limiter.check("app-a", 60, 1_700_000_000).allowed);
        }
        assert!(!limiter.check("app-a", 60, 1_700_000_000).allowed);
        assert!(limiter.check("app-b", 60, 1_700_000_000).allowed);
        assert!(limiter.check("app-a", 60, 1_700_000_000 + 60).allowed);
    }
}
