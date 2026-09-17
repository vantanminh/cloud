use std::time::Duration;

use anyhow::{Context, Result, bail};
use k8s_openapi::api::core::v1::Pod;
use kube::{
    Client,
    api::{
        Api, ApiResource, DeleteParams, DynamicObject, GroupVersionKind, ListParams, Patch,
        PatchParams, Request as KubeRequest,
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::{
    cluster::{
        ClusterSpec, KubernetesCapacityUnavailable, PROJECT_NETWORK_APP_ALIAS,
        PROJECT_NETWORK_REDIS_ALIAS, PROVIDER_KUBERNETES, ProvisionedCluster, RuntimeMetrics,
    },
    config::Config,
    limits::{
        RESOURCE_VOLUME_LIMIT_BYTES, kubernetes_resource_requirements, kubernetes_storage_request,
    },
};

const POSTGRES_PORT: u16 = 5432;
const FIELD_MANAGER: &str = "knotree-api";

pub async fn provision(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    let client = Client::try_default()
        .await
        .context("could not connect to the Kubernetes API")?;
    let namespace = config.database_cluster_namespace.clone();
    let name = cluster_name(spec);
    let volume_name = format!("data-{name}-0");
    let secret_name = format!("{name}-credentials");
    let labels = labels(spec);

    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("", "v1", "Secret"),
        &secret_name,
        json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {
                "name": secret_name,
                "namespace": namespace,
                "labels": labels,
            },
            "type": "Opaque",
            "stringData": {
                "POSTGRES_DB": spec.database_name,
                "POSTGRES_USER": spec.role_name,
                "POSTGRES_PASSWORD": spec.password,
            },
        }),
    )
    .await?;

    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("", "v1", "Service"),
        &name,
        service_manifest(
            &namespace,
            &name,
            &labels,
            &config.database_cluster_service_type,
        ),
    )
    .await?;

    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("apps", "v1", "StatefulSet"),
        &name,
        stateful_set_manifest(config, spec, &namespace, &name, &secret_name, &labels),
    )
    .await?;

    let stateful_sets: Api<DynamicObject> = namespaced_api(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("apps", "v1", "StatefulSet"),
    );
    let pods: Api<Pod> = Api::namespaced(client.clone(), &namespace);
    let wait_result = wait_for_stateful_set(
        &stateful_sets,
        &pods,
        &name,
        config.database_cluster_startup_timeout_seconds,
    )
    .await;
    if let Err(error) = wait_result {
        if error
            .downcast_ref::<KubernetesCapacityUnavailable>()
            .is_some()
        {
            if let Err(cleanup_error) = stateful_sets.delete(&name, &DeleteParams::default()).await
            {
                tracing::warn!(
                    namespace,
                    stateful_set = name,
                    error = %cleanup_error,
                    "could not remove unschedulable PostgreSQL StatefulSet"
                );
            }
        }
        return Err(error);
    }

    let internal_host = format!("{name}.{namespace}.svc.cluster.local");
    let (public_host, public_port) = public_endpoint(config, &client, &namespace, &name).await?;

    Ok(ProvisionedCluster {
        provider: PROVIDER_KUBERNETES.to_owned(),
        name,
        namespace: Some(namespace),
        volume: Some(volume_name),
        internal_host,
        internal_port: POSTGRES_PORT,
        public_host,
        public_port,
        network_name: None,
    })
}

fn cluster_name(spec: &ClusterSpec) -> String {
    format!("knotree-pg-{}", spec.project_id.simple())
}

fn labels(spec: &ClusterSpec) -> Value {
    json!({
        "app.knotree.com/managed-by": "knotree-api",
        "app.knotree.com/component": "postgres-cluster",
        "app.knotree.com/project-id": spec.project_id.to_string(),
    })
}

fn service_manifest(namespace: &str, name: &str, labels: &Value, service_type: &str) -> Value {
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": labels,
        },
        "spec": {
            "type": service_type,
            "selector": labels,
            "ports": [{
                "name": "postgres",
                "port": POSTGRES_PORT,
                "targetPort": POSTGRES_PORT,
                "protocol": "TCP",
            }],
        },
    })
}

fn stateful_set_manifest(
    config: &Config,
    spec: &ClusterSpec,
    namespace: &str,
    name: &str,
    secret_name: &str,
    labels: &Value,
) -> Value {
    json!({
        "apiVersion": "apps/v1",
        "kind": "StatefulSet",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": labels,
        },
        "spec": {
            "serviceName": name,
            "replicas": 1,
            "selector": { "matchLabels": labels },
            "template": {
                "metadata": { "labels": labels },
                "spec": {
                    "automountServiceAccountToken": false,
                    "securityContext": {
                        "runAsNonRoot": true,
                        "seccompProfile": { "type": "RuntimeDefault" },
                    },
                    "containers": [{
                        "name": "postgres",
                        "image": config.database_cluster_image,
                        "imagePullPolicy": "IfNotPresent",
                        "ports": [{
                            "name": "postgres",
                            "containerPort": POSTGRES_PORT,
                            "protocol": "TCP",
                        }],
                        "env": [
                            {
                                "name": "POSTGRES_DB",
                                "valueFrom": { "secretKeyRef": { "name": secret_name, "key": "POSTGRES_DB" } },
                            },
                            {
                                "name": "POSTGRES_USER",
                                "valueFrom": { "secretKeyRef": { "name": secret_name, "key": "POSTGRES_USER" } },
                            },
                            {
                                "name": "POSTGRES_PASSWORD",
                                "valueFrom": { "secretKeyRef": { "name": secret_name, "key": "POSTGRES_PASSWORD" } },
                            },
                            { "name": "PGDATA", "value": "/var/lib/postgresql/data/pgdata" },
                        ],
                        "readinessProbe": {
                            "exec": { "command": ["pg_isready", "-U", spec.role_name, "-d", spec.database_name] },
                            "periodSeconds": 5,
                            "timeoutSeconds": 3,
                            "failureThreshold": 12,
                        },
                        "livenessProbe": {
                            "exec": { "command": ["pg_isready", "-U", spec.role_name, "-d", spec.database_name] },
                            "initialDelaySeconds": 30,
                            "periodSeconds": 20,
                            "timeoutSeconds": 5,
                            "failureThreshold": 6,
                        },
                        "resources": kubernetes_resource_requirements(),
                        "volumeMounts": [{
                            "name": "data",
                            "mountPath": "/var/lib/postgresql/data",
                        }],
                    }],
                },
            },
            "volumeClaimTemplates": [{
                "metadata": { "name": "data", "labels": labels },
                "spec": {
                    "accessModes": ["ReadWriteOnce"],
                    "resources": kubernetes_storage_request(),
                },
            }],
        },
    })
}

#[derive(Debug, Clone)]
pub struct AppWorkloadSpec {
    pub project_id: uuid::Uuid,
    pub service_id: uuid::Uuid,
    pub image: String,
    pub app_port: u16,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ProvisionedApp {
    pub name: String,
    pub namespace: String,
    pub host: String,
    pub port: u16,
}

/// Read complete runtime counters for a Kubernetes App service.
///
/// The control-plane API runs in the same namespace as tenant workloads and
/// stores the Deployment name in `container_name` for Kubernetes services. A
/// live pod is selected by that Deployment prefix, then the kubelet summary
/// and cAdvisor endpoints are used for CPU, memory, ephemeral storage,
/// network, and block I/O counters.
pub async fn collect_app_service_runtime_metrics(
    config: &Config,
    deployment_name: &str,
) -> Result<RuntimeMetrics> {
    collect_runtime_metrics(config, deployment_name, "app").await
}

/// Read complete runtime counters for a Kubernetes workload.
pub async fn collect_runtime_metrics(
    config: &Config,
    workload_name: &str,
    container_name: &str,
) -> Result<RuntimeMetrics> {
    let client = Client::try_default()
        .await
        .context("could not connect to the Kubernetes API for runtime metrics")?;
    let namespace = config.database_cluster_namespace.as_str();
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    let pod_prefix = format!("{workload_name}-");
    let pod = pods
        .list(&ListParams::default().labels("app.knotree.com/managed-by=knotree-api"))
        .await
        .context("could not list Kubernetes workload pods")?
        .items
        .into_iter()
        .find(|pod| {
            pod.metadata
                .name
                .as_deref()
                .is_some_and(|name| name.starts_with(&pod_prefix))
                && pod
                    .status
                    .as_ref()
                    .and_then(|status| status.phase.as_deref())
                    == Some("Running")
        })
        .with_context(|| {
            format!("could not find a running Kubernetes workload pod for {workload_name}")
        })?;
    let pod_name = pod
        .metadata
        .name
        .as_deref()
        .context("Kubernetes workload pod has no name")?;
    let node_name = pod
        .spec
        .as_ref()
        .and_then(|spec| spec.node_name.as_deref())
        .context("Kubernetes workload pod has no node name")?;
    let summary = kubelet_stats_summary(&client, node_name).await?;
    let pod_stats = summary
        .pods
        .iter()
        .find(|stats| stats.pod_ref.namespace == namespace && stats.pod_ref.name == pod_name)
        .with_context(|| format!("kubelet has no stats for pod {namespace}/{pod_name}"))?;
    let container_stats = pod_stats
        .containers
        .iter()
        .find(|container| container.name == container_name)
        .or_else(|| pod_stats.containers.first())
        .with_context(|| format!("kubelet has no stats for container {container_name}"))?;

    let cpu_cores = container_stats
        .cpu
        .as_ref()
        .and_then(|cpu| cpu.usage_nano_cores)
        .map(|nano_cores| nano_cores as f64 / 1_000_000_000.0)
        .unwrap_or_default();
    let memory_used_bytes = container_stats
        .memory
        .as_ref()
        .and_then(|memory| memory.working_set_bytes.or(memory.usage_bytes))
        .map(i64_from_u64)
        .transpose()?
        .unwrap_or_default();

    let cpu_limit_cores = pod_container_limit(&pod, container_name, "cpu");
    let memory_limit_bytes = pod_container_limit(&pod, container_name, "memory")
        .map(|value| parse_bytes_value(value, "memory limit"))
        .transpose()?;
    let cpu_percent = cpu_limit_cores
        .filter(|limit| *limit > 0.0)
        .map(|limit| (cpu_cores / limit) * 100.0)
        .unwrap_or(cpu_cores * 100.0);
    if !cpu_percent.is_finite() || cpu_percent < 0.0 {
        bail!("Kubernetes returned an invalid CPU percentage");
    }

    let volume_used_bytes = pod_storage_bytes(pod_stats, container_stats);
    let network_receive_bytes = pod_stats
        .network
        .as_ref()
        .and_then(|network| network.rx_bytes)
        .map(i64_from_u64)
        .transpose()?
        .unwrap_or_default();
    let network_transmit_bytes = pod_stats
        .network
        .as_ref()
        .and_then(|network| network.tx_bytes)
        .map(i64_from_u64)
        .transpose()?
        .unwrap_or_default();
    let (disk_read_bytes, disk_write_bytes) =
        collect_cadvisor_io_metrics(&client, node_name, pod_name, container_name)
            .await
            .unwrap_or_else(|error| {
                tracing::debug!(
                    node = node_name,
                    pod = pod_name,
                    container = container_name,
                    error = %error,
                    "could not read kubelet cAdvisor disk metrics; using kubelet I/O counters"
                );
                (
                    container_stats
                        .io
                        .as_ref()
                        .and_then(|io| io.read_bytes)
                        .and_then(|value| i64::try_from(value).ok())
                        .unwrap_or_default(),
                    container_stats
                        .io
                        .as_ref()
                        .and_then(|io| io.write_bytes)
                        .and_then(|value| i64::try_from(value).ok())
                        .unwrap_or_default(),
                )
            });

    Ok(RuntimeMetrics {
        cpu_percent: Some(cpu_percent.min(100.0)),
        memory_used_bytes: Some(memory_used_bytes),
        memory_limit_bytes,
        volume_used_bytes: Some(volume_used_bytes.unwrap_or_default()),
        volume_capacity_bytes: Some(RESOURCE_VOLUME_LIMIT_BYTES),
        network_receive_bytes: Some(network_receive_bytes),
        network_transmit_bytes: Some(network_transmit_bytes),
        disk_read_bytes: Some(disk_read_bytes),
        disk_write_bytes: Some(disk_write_bytes),
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletStatsSummary {
    #[serde(default)]
    pods: Vec<KubeletPodStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletPodStats {
    pod_ref: KubeletPodReference,
    #[serde(default)]
    containers: Vec<KubeletContainerStats>,
    #[serde(rename = "ephemeral-storage")]
    ephemeral_storage: Option<KubeletFsStats>,
    #[serde(default)]
    volume: Vec<KubeletFsStats>,
    network: Option<KubeletNetworkStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletPodReference {
    name: String,
    namespace: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletContainerStats {
    name: String,
    cpu: Option<KubeletCpuStats>,
    memory: Option<KubeletMemoryStats>,
    io: Option<KubeletIoStats>,
    rootfs: Option<KubeletFsStats>,
    logs: Option<KubeletFsStats>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletCpuStats {
    usage_nano_cores: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletMemoryStats {
    working_set_bytes: Option<u64>,
    usage_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletIoStats {
    read_bytes: Option<u64>,
    write_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletFsStats {
    used_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KubeletNetworkStats {
    rx_bytes: Option<u64>,
    tx_bytes: Option<u64>,
}

async fn kubelet_stats_summary(client: &Client, node_name: &str) -> Result<KubeletStatsSummary> {
    let request = KubeRequest::new(format!("/api/v1/nodes/{node_name}/proxy/stats/summary"))
        .list(&ListParams::default())?;
    client
        .request(request)
        .await
        .with_context(|| format!("could not read kubelet stats summary for node {node_name}"))
}

async fn collect_cadvisor_io_metrics(
    client: &Client,
    node_name: &str,
    pod_name: &str,
    container_name: &str,
) -> Result<(i64, i64)> {
    let request = KubeRequest::new(format!("/api/v1/nodes/{node_name}/proxy/metrics/cadvisor"))
        .list(&ListParams::default())?;
    let payload = client
        .request_text(request)
        .await
        .with_context(|| format!("could not read kubelet cAdvisor metrics for node {node_name}"))?;
    parse_cadvisor_io_metrics(&payload, pod_name, container_name)
}

fn pod_storage_bytes(
    pod_stats: &KubeletPodStats,
    container_stats: &KubeletContainerStats,
) -> Option<i64> {
    let volume_bytes = pod_stats
        .volume
        .iter()
        .filter_map(|stats| stats.used_bytes)
        .try_fold(0i64, |total, value| {
            i64_from_u64(value)
                .ok()
                .and_then(|value| total.checked_add(value))
        });
    let ephemeral_bytes = pod_stats
        .ephemeral_storage
        .as_ref()
        .and_then(|storage| storage.used_bytes)
        .or_else(|| {
            [
                container_stats.rootfs.as_ref(),
                container_stats.logs.as_ref(),
            ]
            .into_iter()
            .filter_map(|stats| stats.and_then(|stats| stats.used_bytes))
            .try_fold(0u64, |total, value| total.checked_add(value))
        });
    match (
        ephemeral_bytes.map(i64_from_u64).transpose().ok()?,
        volume_bytes,
    ) {
        (Some(ephemeral), Some(volume)) => ephemeral.checked_add(volume),
        (Some(bytes), None) | (None, Some(bytes)) => Some(bytes),
        (None, None) => None,
    }
}

fn i64_from_u64(value: u64) -> Result<i64> {
    i64::try_from(value).context("Kubernetes runtime metric exceeded i64 range")
}

fn parse_cadvisor_io_metrics(
    payload: &str,
    pod_name: &str,
    container_name: &str,
) -> Result<(i64, i64)> {
    let mut read_bytes = 0i64;
    let mut write_bytes = 0i64;
    let mut matched = false;
    for line in payload.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (metric, value_token) = if let Some(close_brace) = line.rfind('}') {
            let (metric, remainder) = line.split_at(close_brace + 1);
            (
                metric,
                remainder
                    .split_whitespace()
                    .next()
                    .context("cAdvisor returned a metric without a value")?,
            )
        } else {
            let (metric, remainder) = line
                .split_once(char::is_whitespace)
                .context("cAdvisor returned a malformed metric")?;
            (
                metric,
                remainder
                    .split_whitespace()
                    .next()
                    .context("cAdvisor returned a metric without a value")?,
            )
        };
        let value = value_token.parse::<f64>()?;
        let (name, labels) = metric
            .split_once('{')
            .map(|(name, labels)| (name, labels.strip_suffix('}').unwrap_or(labels)))
            .unwrap_or((metric, ""));
        if prometheus_label_value(labels, "pod").as_deref() != Some(pod_name)
            || prometheus_label_value(labels, "container").as_deref() != Some(container_name)
        {
            continue;
        }
        let bytes = parse_prometheus_counter(value)?;
        match name {
            "container_fs_reads_bytes_total" => {
                read_bytes = read_bytes
                    .checked_add(bytes)
                    .context("cAdvisor read metric overflowed")?;
                matched = true;
            }
            "container_fs_writes_bytes_total" => {
                write_bytes = write_bytes
                    .checked_add(bytes)
                    .context("cAdvisor write metric overflowed")?;
                matched = true;
            }
            _ => {}
        }
    }
    if !matched {
        bail!("cAdvisor returned no disk metrics for pod {pod_name} container {container_name}");
    }
    Ok((read_bytes, write_bytes))
}

fn prometheus_label_value(labels: &str, key: &str) -> Option<String> {
    labels.split(',').find_map(|label| {
        let (label_key, raw_value) = label.trim().split_once('=')?;
        if label_key != key {
            return None;
        }
        let raw_value = raw_value.strip_prefix('"')?.strip_suffix('"')?;
        let mut value = String::with_capacity(raw_value.len());
        let mut escaped = false;
        for character in raw_value.chars() {
            if escaped {
                value.push(match character {
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    other => other,
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else {
                value.push(character);
            }
        }
        (!escaped).then_some(value)
    })
}

fn parse_prometheus_counter(value: f64) -> Result<i64> {
    if !value.is_finite() || value < 0.0 || value > i64::MAX as f64 {
        bail!("cAdvisor returned an invalid disk counter");
    }
    Ok(value.round() as i64)
}

fn pod_container_limit(pod: &Pod, container_name: &str, resource_name: &str) -> Option<f64> {
    pod.spec
        .as_ref()?
        .containers
        .iter()
        .find(|container| container.name == container_name)
        .or_else(|| pod.spec.as_ref()?.containers.first())
        .and_then(|container| container.resources.as_ref())
        .and_then(|resources| resources.limits.as_ref())
        .and_then(|limits| limits.get(resource_name))
        .and_then(|quantity| parse_kubernetes_quantity(&quantity.0).ok())
}

fn parse_kubernetes_bytes(input: &str) -> Result<i64> {
    parse_kubernetes_quantity(input).and_then(|value| parse_bytes_value(value, "metric"))
}

fn parse_bytes_value(value: f64, label: &str) -> Result<i64> {
    if !value.is_finite() || value < 0.0 || value > i64::MAX as f64 {
        bail!("Kubernetes returned an invalid {label} quantity");
    }
    Ok(value.round() as i64)
}

fn parse_kubernetes_quantity(input: &str) -> Result<f64> {
    let raw = input.trim();
    if raw.is_empty() {
        bail!("Kubernetes returned an empty quantity");
    }
    if let Ok(value) = raw.parse::<f64>() {
        return validate_kubernetes_quantity(value);
    }

    const SUFFIXES: [(&str, f64); 15] = [
        ("Ei", 1.1529215046068469e18),
        ("Pi", 1.125899906842624e15),
        ("Ti", 1.099511627776e12),
        ("Gi", 1.073741824e9),
        ("Mi", 1.048576e6),
        ("Ki", 1.024e3),
        ("n", 1e-9),
        ("u", 1e-6),
        ("m", 1e-3),
        ("k", 1e3),
        ("M", 1e6),
        ("G", 1e9),
        ("T", 1e12),
        ("P", 1e15),
        ("E", 1e18),
    ];
    let (number, multiplier) = SUFFIXES
        .iter()
        .find_map(|(suffix, multiplier)| {
            raw.strip_suffix(suffix).map(|number| (number, *multiplier))
        })
        .context("Kubernetes returned an unknown quantity suffix")?;
    let value = number.parse::<f64>()? * multiplier;
    validate_kubernetes_quantity(value)
}

fn validate_kubernetes_quantity(value: f64) -> Result<f64> {
    if !value.is_finite() || value < 0.0 {
        bail!("Kubernetes returned an invalid quantity");
    }
    Ok(value)
}

pub fn app_resource_name(service_id: uuid::Uuid) -> String {
    format!("knotree-app-{}", service_id.simple())
}

pub fn redis_resource_name(project_id: uuid::Uuid) -> String {
    format!("knotree-redis-{}", project_id.simple())
}

pub fn redis_service_dns(project_id: uuid::Uuid, namespace: &str) -> String {
    format!(
        "{}.{}.svc.cluster.local",
        redis_resource_name(project_id),
        namespace
    )
}

pub fn redis_service_manifest(project_id: uuid::Uuid, namespace: &str, labels: &Value) -> Value {
    let name = redis_resource_name(project_id);
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": { "name": name, "namespace": namespace, "labels": labels },
        "spec": {
            "type": "ClusterIP",
            "selector": labels,
            "ports": [{ "name": "redis", "port": 6379, "targetPort": 6379, "protocol": "TCP" }],
        },
    })
}

pub fn app_deployment_manifest(
    spec: &AppWorkloadSpec,
    namespace: &str,
    image_pull_secret: Option<&str>,
) -> Value {
    let name = app_resource_name(spec.service_id);
    let labels = json!({
        "app.knotree.com/managed-by": "knotree-api",
        "app.knotree.com/component": "app-service",
        "app.knotree.com/project-id": spec.project_id.to_string(),
        "app.knotree.com/app-service-id": spec.service_id.to_string(),
        "app.knotree.com/network-alias": PROJECT_NETWORK_APP_ALIAS,
    });
    let env = spec
        .env
        .iter()
        .map(|(key, value)| json!({ "name": key, "value": value }))
        .collect::<Vec<_>>();
    let mut pod_spec = json!({
        "automountServiceAccountToken": false,
        "securityContext": {
            "runAsNonRoot": true,
            "seccompProfile": { "type": "RuntimeDefault" },
        },
        "containers": [{
            "name": "app",
            "image": spec.image,
            "imagePullPolicy": "IfNotPresent",
            "ports": [{
                "name": "http",
                "containerPort": spec.app_port,
                "protocol": "TCP",
            }],
            "env": env,
            "resources": kubernetes_resource_requirements(),
            "securityContext": {
                "allowPrivilegeEscalation": false,
                "readOnlyRootFilesystem": false,
                "capabilities": { "drop": ["ALL"] },
            },
        }],
    });
    if let Some(secret) = image_pull_secret.filter(|value| !value.is_empty()) {
        pod_spec["imagePullSecrets"] = json!([{ "name": secret }]);
    }
    json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": { "name": name, "namespace": namespace, "labels": labels },
        "spec": {
            "replicas": 1,
            "selector": { "matchLabels": labels },
            "template": {
                "metadata": { "labels": labels },
                "spec": pod_spec,
            },
        },
    })
}

pub fn app_service_manifest(spec: &AppWorkloadSpec, namespace: &str) -> Value {
    let name = app_resource_name(spec.service_id);
    let labels = json!({
        "app.knotree.com/managed-by": "knotree-api",
        "app.knotree.com/component": "app-service",
        "app.knotree.com/project-id": spec.project_id.to_string(),
        "app.knotree.com/app-service-id": spec.service_id.to_string(),
    });
    json!({
        "apiVersion": "v1",
        "kind": "Service",
        "metadata": { "name": name, "namespace": namespace, "labels": labels },
        "spec": {
            "type": "ClusterIP",
            "selector": labels,
            "ports": [{
                "name": "http",
                "port": spec.app_port,
                "targetPort": spec.app_port,
                "protocol": "TCP",
            }],
        },
    })
}

pub fn redis_stateful_set_manifest(config: &Config, spec: &ClusterSpec, namespace: &str) -> Value {
    let name = redis_resource_name(spec.project_id);
    let labels = json!({
        "app.knotree.com/managed-by": "knotree-api",
        "app.knotree.com/component": "redis",
        "app.knotree.com/project-id": spec.project_id.to_string(),
        "app.knotree.com/network-alias": PROJECT_NETWORK_REDIS_ALIAS,
    });
    json!({
        "apiVersion": "apps/v1",
        "kind": "StatefulSet",
        "metadata": { "name": name, "namespace": namespace, "labels": labels },
        "spec": {
            "serviceName": name,
            "replicas": 1,
            "selector": { "matchLabels": labels },
            "template": {
                "metadata": { "labels": labels },
                "spec": {
                    "automountServiceAccountToken": false,
                    "containers": [{
                        "name": "redis",
                        "image": config.redis_cluster_image,
                        "imagePullPolicy": "IfNotPresent",
                        "args": [
                            "redis-server",
                            "--appendonly", "yes",
                            "--requirepass", spec.password,
                            "--maxmemory", "768mb",
                            "--maxmemory-policy", "allkeys-lru",
                        ],
                        "ports": [{ "name": "redis", "containerPort": 6379, "protocol": "TCP" }],
                        "resources": kubernetes_resource_requirements(),
                        "volumeMounts": [{ "name": "data", "mountPath": "/data" }],
                    }],
                },
            },
            "volumeClaimTemplates": [{
                "metadata": { "name": "data", "labels": labels },
                "spec": {
                    "accessModes": ["ReadWriteOnce"],
                    "resources": kubernetes_storage_request(),
                },
            }],
        },
    })
}

pub fn project_network_policy_manifest(project_id: uuid::Uuid, namespace: &str) -> Value {
    let name = format!("knotree-net-{}", &project_id.simple().to_string()[..20]);
    json!({
        "apiVersion": "networking.k8s.io/v1",
        "kind": "NetworkPolicy",
        "metadata": { "name": name, "namespace": namespace },
        "spec": {
            "podSelector": {
                "matchLabels": { "app.knotree.com/project-id": project_id.to_string() }
            },
            "policyTypes": ["Ingress"],
            "ingress": [
                {
                    "from": [
                        { "podSelector": { "matchLabels": { "app.knotree.com/project-id": project_id.to_string() } } },
                        { "podSelector": { "matchLabels": { "app.kubernetes.io/component": "api" } } },
                        { "podSelector": { "matchLabels": { "app.kubernetes.io/component": "apps-kong" } } },
                    ]
                }
            ],
        },
    })
}

pub async fn provision_app(config: &Config, spec: &AppWorkloadSpec) -> Result<ProvisionedApp> {
    let client = Client::try_default()
        .await
        .context("could not connect to the Kubernetes API")?;
    let namespace = config.database_cluster_namespace.clone();
    let name = app_resource_name(spec.service_id);
    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("", "v1", "Service"),
        &name,
        app_service_manifest(spec, &namespace),
    )
    .await?;
    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("apps", "v1", "Deployment"),
        &name,
        app_deployment_manifest(
            spec,
            &namespace,
            config.app_service_image_pull_secret.as_deref(),
        ),
    )
    .await?;
    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("networking.k8s.io", "v1", "NetworkPolicy"),
        &format!(
            "knotree-net-{}",
            &spec.project_id.simple().to_string()[..20]
        ),
        project_network_policy_manifest(spec.project_id, &namespace),
    )
    .await?;
    let deployments: Api<DynamicObject> = namespaced_api(
        client,
        &namespace,
        GroupVersionKind::gvk("apps", "v1", "Deployment"),
    );
    wait_for_deployment(
        &deployments,
        &name,
        config.database_cluster_startup_timeout_seconds,
    )
    .await?;
    Ok(ProvisionedApp {
        host: format!("{name}.{namespace}.svc.cluster.local"),
        port: spec.app_port,
        name,
        namespace,
    })
}

pub async fn provision_redis(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    let client = Client::try_default()
        .await
        .context("could not connect to the Kubernetes API")?;
    let namespace = config.database_cluster_namespace.clone();
    let name = redis_resource_name(spec.project_id);
    let labels = json!({
        "app.knotree.com/managed-by": "knotree-api",
        "app.knotree.com/component": "redis",
        "app.knotree.com/project-id": spec.project_id.to_string(),
    });
    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("", "v1", "Service"),
        &name,
        redis_service_manifest(spec.project_id, &namespace, &labels),
    )
    .await?;
    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("apps", "v1", "StatefulSet"),
        &name,
        redis_stateful_set_manifest(config, spec, &namespace),
    )
    .await?;
    apply_resource(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("networking.k8s.io", "v1", "NetworkPolicy"),
        &format!(
            "knotree-net-{}",
            &spec.project_id.simple().to_string()[..20]
        ),
        project_network_policy_manifest(spec.project_id, &namespace),
    )
    .await?;
    let stateful_sets: Api<DynamicObject> = namespaced_api(
        client.clone(),
        &namespace,
        GroupVersionKind::gvk("apps", "v1", "StatefulSet"),
    );
    let pods: Api<Pod> = Api::namespaced(client, &namespace);
    let wait_result = wait_for_stateful_set(
        &stateful_sets,
        &pods,
        &name,
        config.database_cluster_startup_timeout_seconds,
    )
    .await;
    if let Err(error) = wait_result {
        if error
            .downcast_ref::<KubernetesCapacityUnavailable>()
            .is_some()
        {
            if let Err(cleanup_error) = stateful_sets.delete(&name, &DeleteParams::default()).await
            {
                tracing::warn!(
                    namespace,
                    stateful_set = name,
                    error = %cleanup_error,
                    "could not remove unschedulable Redis StatefulSet"
                );
            }
        }
        return Err(error);
    }
    Ok(ProvisionedCluster {
        provider: PROVIDER_KUBERNETES.to_owned(),
        name: name.clone(),
        namespace: Some(namespace.clone()),
        volume: Some(format!("data-{name}-0")),
        internal_host: redis_service_dns(spec.project_id, &namespace),
        internal_port: 6379,
        public_host: None,
        public_port: None,
        network_name: None,
    })
}

pub async fn pod_logs(namespace: &str, name: &str, tail_lines: i64) -> Result<Vec<String>> {
    let client = Client::try_default()
        .await
        .context("could not connect to the Kubernetes API")?;
    let pods: Api<k8s_openapi::api::core::v1::Pod> = Api::namespaced(client, namespace);
    let list = pods
        .list(&kube::api::ListParams::default())
        .await
        .context("could not list Kubernetes pods")?;
    let pod_name = list
        .items
        .into_iter()
        .filter_map(|pod| pod.metadata.name)
        .find(|pod_name| pod_name.starts_with(name))
        .context("could not find a pod for the workload")?;
    let params = kube::api::LogParams {
        tail_lines: Some(tail_lines),
        timestamps: true,
        ..Default::default()
    };
    let logs = pods
        .logs(&pod_name, &params)
        .await
        .context("could not read Kubernetes pod logs")?;
    Ok(logs.lines().map(ToOwned::to_owned).collect())
}

async fn wait_for_deployment(
    api: &Api<DynamicObject>,
    name: &str,
    timeout_seconds: u32,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
    loop {
        let deployment = api
            .get(name)
            .await
            .with_context(|| format!("could not read Kubernetes Deployment {name}"))?;
        let ready_replicas = deployment
            .data
            .get("status")
            .and_then(|status| status.get("readyReplicas"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if ready_replicas >= 1 {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "Kubernetes App service Deployment did not become ready before the startup timeout"
            );
        }
        sleep(Duration::from_secs(2)).await;
    }
}

fn namespaced_api(client: Client, namespace: &str, gvk: GroupVersionKind) -> Api<DynamicObject> {
    let resource = ApiResource::from_gvk(&gvk);
    Api::namespaced_with(client, namespace, &resource)
}

async fn apply_resource(
    client: Client,
    namespace: &str,
    gvk: GroupVersionKind,
    name: &str,
    manifest: Value,
) -> Result<()> {
    let api = namespaced_api(client, namespace, gvk);
    api.patch(
        name,
        &PatchParams::apply(FIELD_MANAGER).force(),
        &Patch::Apply(manifest),
    )
    .await
    .with_context(|| format!("could not apply Kubernetes resource {namespace}/{name}"))?;
    Ok(())
}

async fn wait_for_stateful_set(
    api: &Api<DynamicObject>,
    pods: &Api<Pod>,
    name: &str,
    timeout_seconds: u32,
) -> Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(u64::from(timeout_seconds));
    loop {
        let stateful_set = api
            .get(name)
            .await
            .with_context(|| format!("could not read Kubernetes StatefulSet {name}"))?;
        let ready_replicas = stateful_set
            .data
            .get("status")
            .and_then(|status| status.get("readyReplicas"))
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if ready_replicas >= 1 {
            return Ok(());
        }
        if pod_is_unschedulable(pods, name).await? {
            return Err(KubernetesCapacityUnavailable.into());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "Kubernetes PostgreSQL StatefulSet did not become ready before the startup timeout"
            );
        }
        sleep(Duration::from_secs(2)).await;
    }
}

async fn pod_is_unschedulable(pods: &Api<Pod>, stateful_set_name: &str) -> Result<bool> {
    let pod_name = format!("{stateful_set_name}-0");
    let Some(pod) = pods
        .get_opt(&pod_name)
        .await
        .with_context(|| format!("could not read Kubernetes pod {pod_name}"))?
    else {
        return Ok(false);
    };

    Ok(pod
        .status
        .and_then(|status| status.conditions)
        .unwrap_or_default()
        .into_iter()
        .any(|condition| {
            condition.type_ == "PodScheduled"
                && condition.status == "False"
                && condition.reason.as_deref() == Some("Unschedulable")
        }))
}

async fn public_endpoint(
    config: &Config,
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<(Option<String>, Option<u16>)> {
    if let Some(host) = config.database_resource_public_host.clone() {
        return Ok((
            Some(host),
            Some(
                config
                    .database_resource_public_port
                    .unwrap_or(POSTGRES_PORT),
            ),
        ));
    }
    if config.database_cluster_service_type != "LoadBalancer" {
        return Ok((None, None));
    }

    let api = namespaced_api(
        client.clone(),
        namespace,
        GroupVersionKind::gvk("", "v1", "Service"),
    );
    let service = api.get(name).await?;
    let ingress = service
        .data
        .get("status")
        .and_then(|status| status.get("loadBalancer"))
        .and_then(|load_balancer| load_balancer.get("ingress"))
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let host = ingress
        .and_then(|item| item.get("hostname").or_else(|| item.get("ip")))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned);
    let public_port = host.as_ref().map(|_| POSTGRES_PORT);
    Ok((host, public_port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn renders_one_stateful_set_and_one_volume_claim_per_project() {
        let config = Config::test_fixture();
        let spec = ClusterSpec {
            project_id: Uuid::nil(),
            database_name: "knotree_db_test".to_owned(),
            role_name: "knotree_role_test".to_owned(),
            password: "secret".to_owned(),
        };
        let manifest = stateful_set_manifest(
            &config,
            &spec,
            "knotree-clusters",
            "knotree-pg-test",
            "knotree-pg-test-credentials",
            &labels(&spec),
        );
        assert_eq!(manifest["kind"], "StatefulSet");
        assert_eq!(manifest["spec"]["replicas"], 1);
        assert_eq!(
            manifest["spec"]["volumeClaimTemplates"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            manifest["spec"]["template"]["spec"]["containers"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let container = &manifest["spec"]["template"]["spec"]["containers"][0];
        assert_eq!(container["resources"]["limits"]["cpu"], "1");
        assert_eq!(container["resources"]["limits"]["memory"], "1Gi");
        assert_eq!(
            manifest["spec"]["volumeClaimTemplates"][0]["spec"]["resources"]["requests"]["storage"],
            "10Gi"
        );
        assert_eq!(
            container["resources"]["limits"]["ephemeral-storage"],
            "10Gi"
        );
    }

    #[test]
    fn app_and_redis_manifests_use_the_same_hard_caps() {
        let config = Config::test_fixture();
        let project_id = Uuid::nil();
        let service_id = Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let app = app_deployment_manifest(
            &AppWorkloadSpec {
                project_id,
                service_id,
                image: "nginx:alpine".to_owned(),
                app_port: 80,
                env: vec![(
                    "DATABASE_URL".to_owned(),
                    "postgres://postgres:5432/db".to_owned(),
                )],
            },
            "knotree-cloud",
            None,
        );
        let redis = redis_stateful_set_manifest(
            &config,
            &ClusterSpec {
                project_id,
                database_name: "redis".to_owned(),
                role_name: "default".to_owned(),
                password: "secret".to_owned(),
            },
            "knotree-cloud",
        );
        for manifest in [&app, &redis] {
            let container = &manifest["spec"]["template"]["spec"]["containers"][0];
            assert_eq!(container["resources"]["limits"]["cpu"], "1");
            assert_eq!(container["resources"]["limits"]["memory"], "1Gi");
            assert_eq!(
                container["resources"]["limits"]["ephemeral-storage"],
                "10Gi"
            );
        }
        assert_eq!(
            redis["spec"]["volumeClaimTemplates"][0]["spec"]["resources"]["requests"]["storage"],
            "10Gi"
        );
        assert_eq!(crate::limits::TENANT_RESOURCE_CAPS.cpu, "1");
        let redis_name = redis_resource_name(project_id);
        assert_eq!(redis_name, format!("knotree-redis-{}", project_id.simple()));
        assert_ne!(redis_name, crate::cluster::PROJECT_NETWORK_REDIS_ALIAS);
        assert_eq!(
            redis_service_dns(project_id, "knotree-cloud"),
            format!("{redis_name}.knotree-cloud.svc.cluster.local")
        );
        assert_eq!(redis["metadata"]["name"], redis_name);
        assert_eq!(redis["spec"]["serviceName"], redis_name);
        let service = redis_service_manifest(
            project_id,
            "knotree-cloud",
            &serde_json::json!({ "app.knotree.com/component": "redis" }),
        );
        assert_eq!(service["metadata"]["name"], redis_name);
        assert_ne!(
            service["metadata"]["name"],
            crate::cluster::PROJECT_NETWORK_REDIS_ALIAS
        );
    }

    #[test]
    fn parses_kubernetes_runtime_quantities() {
        let cpu_nanos = parse_kubernetes_quantity("2164329n").unwrap();
        assert!((cpu_nanos - 0.002164329).abs() < 1e-12);
        assert_eq!(parse_kubernetes_quantity("500m").unwrap(), 0.5);
        assert_eq!(parse_kubernetes_bytes("6100Ki").unwrap(), 6_246_400);
        assert_eq!(parse_kubernetes_bytes("1Gi").unwrap(), 1_073_741_824);
        assert!(parse_kubernetes_quantity("-1").is_err());
        assert!(parse_kubernetes_quantity("wat").is_err());
    }

    #[test]
    fn parses_kubelet_summary_runtime_fields() {
        let summary = serde_json::from_value::<KubeletStatsSummary>(serde_json::json!({
            "pods": [{
                "podRef": {
                    "name": "workload-abc123",
                    "namespace": "knotree-cloud"
                },
                "containers": [{
                    "name": "app",
                    "cpu": { "usageNanoCores": 500000000 },
                    "memory": { "workingSetBytes": 4096 },
                    "io": { "readBytes": 30, "writeBytes": 40 },
                    "rootfs": { "usedBytes": 100 },
                    "logs": { "usedBytes": 20 }
                }],
                "ephemeral-storage": { "usedBytes": 2048 },
                "volume": [{ "name": "cache", "usedBytes": 10 }],
                "network": { "rxBytes": 500, "txBytes": 600 }
            }]
        }))
        .unwrap();
        let pod = &summary.pods[0];
        assert_eq!(pod.pod_ref.name, "workload-abc123");
        assert_eq!(pod.pod_ref.namespace, "knotree-cloud");
        assert_eq!(
            pod.ephemeral_storage.as_ref().unwrap().used_bytes,
            Some(2048)
        );
        assert_eq!(pod.network.as_ref().unwrap().rx_bytes, Some(500));
        assert_eq!(
            pod.containers[0].cpu.as_ref().unwrap().usage_nano_cores,
            Some(500000000)
        );
        assert_eq!(
            pod.containers[0].memory.as_ref().unwrap().working_set_bytes,
            Some(4096)
        );
        assert_eq!(pod.containers[0].io.as_ref().unwrap().write_bytes, Some(40));
        assert_eq!(pod_storage_bytes(pod, &pod.containers[0]), Some(2058));
    }

    #[test]
    fn parses_cadvisor_disk_counters_for_target_container() {
        let payload = concat!(
            "# HELP container_fs_reads_bytes_total read bytes\n",
            "cadvisor_version_info{cadvisorRevision=\"\",cadvisorVersion=\"\",dockerVersion=\"\",kernelVersion=\"6.14.0\",osVersion=\"Ubuntu 25.04\"} 1\n",
            "container_fs_reads_bytes_total{container=\"app\",device=\"/dev/sda\",namespace=\"knotree-cloud\",pod=\"workload-abc123\"} 123\n",
            "container_fs_writes_bytes_total{container=\"app\",device=\"/dev/sda\",namespace=\"knotree-cloud\",pod=\"workload-abc123\"} 456\n",
            "container_fs_reads_bytes_total{container=\"app\",device=\"/dev/sda\",namespace=\"knotree-cloud\",pod=\"other-pod\"} 999\n",
        );
        assert_eq!(
            parse_cadvisor_io_metrics(payload, "workload-abc123", "app").unwrap(),
            (123, 456)
        );
    }
}
