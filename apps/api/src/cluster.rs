use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use tokio::{process::Command, time::sleep};
use uuid::Uuid;

use crate::{config::Config, limits};

pub use crate::limits::{
    RESOURCE_CPU_LIMIT, RESOURCE_MEMORY_LIMIT_DOCKER, RESOURCE_MEMORY_LIMIT_KUBERNETES,
    RESOURCE_MEMORY_SWAP_LIMIT_DOCKER, RESOURCE_STORAGE_LIMIT_MESSAGE, RESOURCE_VOLUME_LIMIT_BYTES,
    RESOURCE_VOLUME_LIMIT_DOCKER, RESOURCE_VOLUME_LIMIT_KUBERNETES, docker_resource_limit_args,
    docker_runtime_limit_args,
};

pub const PROVIDER_DOCKER: &str = "docker";
pub const PROVIDER_KUBERNETES: &str = "kubernetes";
pub const PROJECT_NETWORK_POSTGRES_ALIAS: &str = "postgres";
pub const PROJECT_NETWORK_APP_ALIAS: &str = "app";
pub const PROJECT_NETWORK_REDIS_ALIAS: &str = "redis";
pub const POSTGRES_VOLUME_PATH: &str = "/var/lib/postgresql/data";
pub const APP_SERVICE_VOLUME_PATH: &str = "/";
pub const REDIS_VOLUME_PATH: &str = "/data";

/// The Kubernetes scheduler could not place the tenant workload on any node.
///
/// This is deliberately a typed error so API workers can persist an actionable
/// message without leaking scheduler details (node names, taints, or internal
/// workload metadata) to the browser.
#[derive(Debug, thiserror::Error)]
#[error("Kubernetes has no schedulable capacity for this workload")]
pub struct KubernetesCapacityUnavailable;

fn is_running_docker_state(state: &str) -> bool {
    state.trim().eq_ignore_ascii_case("running")
}

pub fn project_network_name(project_id: Uuid) -> String {
    format!("knotree-net-{}", project_id.simple())
}

/// Docker Desktop publishes the development database on IPv4 loopback. Using
/// `localhost` lets some clients try `::1` first, which adds a multi-second
/// connection fallback on hosts where Docker is not listening on IPv6.
pub fn connection_host(provider: &str, host: &str) -> String {
    if provider == PROVIDER_DOCKER && host.trim().eq_ignore_ascii_case("localhost") {
        return "127.0.0.1".to_owned();
    }
    host.to_owned()
}

#[derive(Debug, Clone)]
pub struct ClusterSpec {
    pub project_id: Uuid,
    pub database_name: String,
    pub role_name: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct ProvisionedCluster {
    pub provider: String,
    pub name: String,
    pub namespace: Option<String>,
    pub volume: Option<String>,
    pub internal_host: String,
    pub internal_port: u16,
    pub public_host: Option<String>,
    pub public_port: Option<u16>,
    pub network_name: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeMetrics {
    pub cpu_percent: Option<f64>,
    pub memory_used_bytes: Option<i64>,
    pub memory_limit_bytes: Option<i64>,
    pub volume_used_bytes: Option<i64>,
    pub volume_capacity_bytes: Option<i64>,
    pub network_receive_bytes: Option<i64>,
    pub network_transmit_bytes: Option<i64>,
    pub disk_read_bytes: Option<i64>,
    pub disk_write_bytes: Option<i64>,
}

pub async fn collect_runtime_metrics(
    config: &Config,
    provider: &str,
    cluster_name: Option<&str>,
) -> Result<RuntimeMetrics> {
    match provider {
        PROVIDER_DOCKER => {
            let cluster_name = cluster_name.context("Docker cluster name is missing")?;
            collect_docker_runtime_metrics(config, cluster_name, POSTGRES_VOLUME_PATH).await
        }
        PROVIDER_KUBERNETES => {
            // Kubernetes metrics-server does not expose network or block I/O.
            // Keep these fields explicitly unavailable until a cluster-level
            // metrics adapter is configured instead of returning made-up data.
            Ok(RuntimeMetrics::default())
        }
        provider => bail!("unsupported resource cluster provider: {provider}"),
    }
}

pub async fn collect_app_service_runtime_metrics(
    config: &Config,
    container_name: &str,
) -> Result<RuntimeMetrics> {
    if config.uses_kubernetes_workloads() {
        return super::cluster_kubernetes::collect_app_service_runtime_metrics(
            config,
            container_name,
        )
        .await;
    }
    collect_docker_runtime_metrics(config, container_name, APP_SERVICE_VOLUME_PATH).await
}

async fn collect_docker_runtime_metrics(
    config: &Config,
    cluster_name: &str,
    volume_path: &str,
) -> Result<RuntimeMetrics> {
    let container_state = docker_container_status(config, cluster_name)
        .await?
        .context("Docker container was not found")?;
    if !is_running_docker_state(&container_state) {
        bail!("Docker container is not running (status: {container_state})");
    }
    ensure_docker_runtime_limits(config, cluster_name).await?;
    let stats_output = docker_raw(
        config,
        [
            "stats".to_owned(),
            "--no-stream".to_owned(),
            "--format={{json .}}".to_owned(),
            cluster_name.to_owned(),
        ],
    )
    .await?;
    if !stats_output.status.success() {
        let detail = String::from_utf8_lossy(&stats_output.stderr);
        let detail = detail
            .lines()
            .next()
            .unwrap_or("unknown Docker stats error");
        bail!("could not read Docker container metrics: {detail}");
    }

    let stats_stdout = String::from_utf8_lossy(&stats_output.stdout);
    let stats_line = stats_stdout
        .lines()
        .find(|line| !line.trim().is_empty())
        .context("Docker returned no container metrics")?;
    let mut metrics = parse_docker_stats(stats_line)?;

    if let Ok(volume_metrics) = docker_volume_metrics(config, cluster_name, volume_path).await {
        metrics.volume_used_bytes = Some(volume_metrics.0);
        metrics.volume_capacity_bytes = Some(volume_metrics.1.min(RESOURCE_VOLUME_LIMIT_BYTES));
    }

    Ok(metrics)
}

pub async fn enforce_docker_storage_limit(
    config: &Config,
    container_name: &str,
    volume_path: &str,
) -> Result<bool> {
    let Some(container_state) = docker_container_status(config, container_name).await? else {
        return Ok(false);
    };
    if !is_running_docker_state(&container_state) {
        return Ok(false);
    }

    // Reconcile older containers as well as newly provisioned ones. Docker
    // cannot retrofit a writable-layer quota with `docker update`, so the
    // storage watchdog below remains the fail-safe for legacy containers.
    ensure_docker_runtime_limits(config, container_name).await?;
    let (used_bytes, _) = docker_volume_metrics(config, container_name, volume_path).await?;
    if used_bytes < RESOURCE_VOLUME_LIMIT_BYTES {
        return Ok(false);
    }

    run_docker(
        config,
        [
            "stop".to_owned(),
            "--time".to_owned(),
            "10".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    tracing::warn!(
        container_name,
        used_bytes,
        limit_bytes = RESOURCE_VOLUME_LIMIT_BYTES,
        "Docker resource reached its storage limit and was stopped"
    );
    Ok(true)
}

async fn docker_volume_metrics(
    config: &Config,
    cluster_name: &str,
    volume_path: &str,
) -> Result<(i64, i64)> {
    if volume_path == APP_SERVICE_VOLUME_PATH {
        return docker_app_storage_metrics(config, cluster_name).await;
    }

    let output = docker_raw(
        config,
        [
            "exec".to_owned(),
            cluster_name.to_owned(),
            "df".to_owned(),
            "-Pk".to_owned(),
            volume_path.to_owned(),
        ],
    )
    .await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail
            .lines()
            .next()
            .unwrap_or("unknown Docker filesystem error");
        bail!("could not read Docker volume metrics: {detail}");
    }
    let (filesystem_used_bytes, capacity_bytes) =
        parse_docker_df(&String::from_utf8_lossy(&output.stdout))?;
    let used_bytes = docker_raw(
        config,
        [
            "exec".to_owned(),
            cluster_name.to_owned(),
            "du".to_owned(),
            "-sk".to_owned(),
            volume_path.to_owned(),
        ],
    )
    .await
    .ok()
    .filter(|output| output.status.success())
    .and_then(|output| parse_docker_du(&String::from_utf8_lossy(&output.stdout)).ok())
    .unwrap_or(filesystem_used_bytes);
    Ok((used_bytes, capacity_bytes))
}

async fn docker_app_storage_metrics(config: &Config, container_name: &str) -> Result<(i64, i64)> {
    let output = docker_raw(
        config,
        [
            "inspect".to_owned(),
            "--size".to_owned(),
            "--format={{json .}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail
            .lines()
            .next()
            .unwrap_or("unknown Docker writable layer metrics error");
        bail!("could not read Docker writable layer metrics: {detail}");
    }

    let container = serde_json::from_slice::<Value>(&output.stdout)
        .context("Docker returned invalid container storage data")?;
    let root_used_bytes = container
        .get("SizeRw")
        .and_then(Value::as_i64)
        .context("Docker returned no writable layer metric")?;
    if root_used_bytes < 0 {
        bail!("Docker returned a negative writable layer metric");
    }

    let mut used_bytes = root_used_bytes;
    if let Some(mounts) = container.get("Mounts").and_then(Value::as_array) {
        for destination in mounts.iter().filter_map(|mount| {
            mount
                .get("Destination")
                .and_then(Value::as_str)
                .filter(|destination| *destination != "/")
        }) {
            let mount_usage = docker_raw(
                config,
                [
                    "exec".to_owned(),
                    container_name.to_owned(),
                    "du".to_owned(),
                    "-sk".to_owned(),
                    destination.to_owned(),
                ],
            )
            .await?;
            if !mount_usage.status.success() {
                let detail = String::from_utf8_lossy(&mount_usage.stderr);
                let detail = detail
                    .lines()
                    .next()
                    .unwrap_or("unknown Docker mounted storage metrics error");
                bail!("could not read Docker mounted storage metrics: {detail}");
            }
            used_bytes = used_bytes
                .checked_add(parse_docker_du(&String::from_utf8_lossy(
                    &mount_usage.stdout,
                ))?)
                .context("Docker app service storage metric overflowed")?;
        }
    }

    Ok((used_bytes, limits::TENANT_RESOURCE_CAPS.storage_bytes))
}

pub async fn ensure_docker_runtime_limits(config: &Config, container_name: &str) -> Result<()> {
    let mut args = vec!["update".to_owned()];
    args.extend(limits::docker_runtime_limit_args());
    args.push(container_name.to_owned());
    run_docker(config, args).await.map(|_| ())
}

#[derive(Debug, Deserialize)]
struct DockerStatsLine {
    #[serde(rename = "CPUPerc")]
    cpu_perc: String,
    #[serde(rename = "MemUsage")]
    mem_usage: String,
    #[serde(rename = "NetIO")]
    net_io: String,
    #[serde(rename = "BlockIO")]
    block_io: String,
}

fn parse_docker_stats(input: &str) -> Result<RuntimeMetrics> {
    let stats = serde_json::from_str::<DockerStatsLine>(input)
        .context("Docker returned an invalid container metrics payload")?;
    let (memory_used_bytes, memory_limit_bytes) = parse_docker_pair(&stats.mem_usage)?;
    let (network_receive_bytes, network_transmit_bytes) = parse_docker_pair(&stats.net_io)?;
    let (disk_read_bytes, disk_write_bytes) = parse_docker_pair(&stats.block_io)?;

    Ok(RuntimeMetrics {
        cpu_percent: Some(parse_docker_percent(&stats.cpu_perc)?),
        memory_used_bytes: Some(memory_used_bytes),
        memory_limit_bytes: Some(memory_limit_bytes),
        volume_used_bytes: None,
        volume_capacity_bytes: None,
        network_receive_bytes: Some(network_receive_bytes),
        network_transmit_bytes: Some(network_transmit_bytes),
        disk_read_bytes: Some(disk_read_bytes),
        disk_write_bytes: Some(disk_write_bytes),
    })
}

fn parse_docker_pair(input: &str) -> Result<(i64, i64)> {
    let (left, right) = input
        .split_once('/')
        .context("Docker returned a metric without two values")?;
    Ok((parse_docker_size(left)?, parse_docker_size(right)?))
}

fn parse_docker_percent(input: &str) -> Result<f64> {
    let value = input
        .trim()
        .strip_suffix('%')
        .context("Docker returned a CPU metric without a percent sign")?
        .trim()
        .parse::<f64>()?;
    if !value.is_finite() || value < 0.0 {
        bail!("Docker returned an invalid CPU metric");
    }
    Ok(value)
}

fn parse_docker_size(input: &str) -> Result<i64> {
    let input = input.trim().replace(',', "");
    let split_index = input
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(input.len());
    let number = input[..split_index].parse::<f64>()?;
    let unit = input[split_index..].trim().to_ascii_lowercase();
    let multiplier = match unit.as_str() {
        "" | "b" => 1.0,
        "kb" => 1_000.0,
        "kib" => 1_024.0,
        "mb" => 1_000_000.0,
        "mib" => 1_048_576.0,
        "gb" => 1_000_000_000.0,
        "gib" => 1_073_741_824.0,
        "tb" => 1_000_000_000_000.0,
        "tib" => 1_099_511_627_776.0,
        _ => bail!("Docker returned an unknown byte unit: {unit}"),
    };
    let bytes = number * multiplier;
    if !bytes.is_finite() || bytes < 0.0 || bytes > i64::MAX as f64 {
        bail!("Docker returned an invalid byte metric");
    }
    Ok(bytes.round() as i64)
}

fn parse_docker_df(input: &str) -> Result<(i64, i64)> {
    let fields = input
        .lines()
        .filter_map(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            (fields.len() >= 4 && fields.get(1)?.parse::<i64>().is_ok()).then_some(fields)
        })
        .last()
        .context("Docker returned no filesystem metrics")?;
    let capacity_kib = fields[1].parse::<i64>()?;
    let used_kib = fields[2].parse::<i64>()?;
    if capacity_kib < 0 || used_kib < 0 {
        bail!("Docker returned negative filesystem metrics");
    }
    Ok((
        used_kib
            .checked_mul(1_024)
            .context("Docker volume used metric overflowed")?,
        capacity_kib
            .checked_mul(1_024)
            .context("Docker volume capacity metric overflowed")?,
    ))
}

fn parse_docker_du(input: &str) -> Result<i64> {
    let kib = input
        .split_whitespace()
        .next()
        .context("Docker returned no directory usage metric")?
        .parse::<i64>()?;
    if kib < 0 {
        bail!("Docker returned negative directory usage");
    }
    kib.checked_mul(1_024)
        .context("Docker directory usage metric overflowed")
}

pub async fn provision(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    provision_with_provider(config, &config.database_cluster_provider, spec).await
}

pub async fn provision_with_provider(
    config: &Config,
    provider: &str,
    spec: &ClusterSpec,
) -> Result<ProvisionedCluster> {
    match provider {
        PROVIDER_DOCKER => provision_docker(config, spec).await,
        PROVIDER_KUBERNETES => super::cluster_kubernetes::provision(config, spec).await,
        provider => bail!("unsupported database cluster provider: {provider}"),
    }
}

pub async fn provision_redis(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    match config.database_cluster_provider.as_str() {
        PROVIDER_DOCKER => provision_redis_docker(config, spec).await,
        PROVIDER_KUBERNETES => super::cluster_kubernetes::provision_redis(config, spec).await,
        provider => bail!("unsupported redis cluster provider: {provider}"),
    }
}

async fn provision_redis_docker(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    let cluster_name = format!("knotree-redis-{}", spec.project_id.simple());
    let volume_name = format!("knotree-redis-data-{}", spec.project_id.simple());
    let network_name = ensure_project_network(config, spec.project_id).await?;
    let internal_host = connection_host(PROVIDER_DOCKER, &config.database_resource_host);
    ensure_docker_volume(config, &volume_name).await?;

    let existing_state = docker_inspect(config, &cluster_name).await?;
    if existing_state.is_none() {
        let publish = format!("{}::6379", config.database_cluster_bind_address);
        let mut docker_args = vec![
            "run".to_owned(),
            "--detach".to_owned(),
            "--name".to_owned(),
            cluster_name.clone(),
            "--label".to_owned(),
            "com.knotree.managed-by=knotree-api".to_owned(),
            "--label".to_owned(),
            format!("com.knotree.project-id={}", spec.project_id),
            "--label".to_owned(),
            "com.knotree.resource-type=redis".to_owned(),
            "--restart".to_owned(),
            "unless-stopped".to_owned(),
            "--network".to_owned(),
            network_name.clone(),
            "--network-alias".to_owned(),
            PROJECT_NETWORK_REDIS_ALIAS.to_owned(),
            "--env".to_owned(),
            format!("REDIS_PASSWORD={}", spec.password),
            "--publish".to_owned(),
            publish,
            "--volume".to_owned(),
            format!("{volume_name}:/data"),
        ];
        docker_args.extend(limits::docker_resource_limit_args());
        docker_args.push(config.redis_cluster_image.clone());
        docker_args.extend([
            "redis-server".to_owned(),
            "--appendonly".to_owned(),
            "yes".to_owned(),
            "--requirepass".to_owned(),
            spec.password.clone(),
            "--maxmemory".to_owned(),
            "768mb".to_owned(),
            "--maxmemory-policy".to_owned(),
            "allkeys-lru".to_owned(),
        ]);
        run_docker(config, docker_args).await?;
    } else if existing_state.as_deref() != Some("running") {
        run_docker(config, ["start".to_owned(), cluster_name.clone()]).await?;
        ensure_docker_runtime_limits(config, &cluster_name).await?;
    } else {
        ensure_docker_runtime_limits(config, &cluster_name).await?;
    }

    ensure_docker_network_attachment(
        config,
        &network_name,
        &cluster_name,
        PROJECT_NETWORK_REDIS_ALIAS,
    )
    .await?;
    let port = docker_published_port(config, &cluster_name, 6379).await?;
    Ok(ProvisionedCluster {
        provider: PROVIDER_DOCKER.to_owned(),
        name: cluster_name,
        namespace: None,
        volume: Some(volume_name),
        internal_host: internal_host.clone(),
        internal_port: port,
        public_host: Some(internal_host),
        public_port: Some(port),
        network_name: Some(network_name),
    })
}

async fn provision_docker(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    let cluster_name = format!("knotree-pg-{}", spec.project_id.simple());
    let volume_name = format!("knotree-pg-data-{}", spec.project_id.simple());
    let network_name = ensure_project_network(config, spec.project_id).await?;
    let internal_host = connection_host(PROVIDER_DOCKER, &config.database_resource_host);

    ensure_docker_volume(config, &volume_name).await?;

    let existing_state = docker_inspect(config, &cluster_name).await?;
    let recreate_existing = if existing_state.is_some() {
        !docker_storage_limit_configured(config, &cluster_name).await?
    } else {
        false
    };
    if recreate_existing {
        // The named volume is intentionally kept so an upgrade from an older
        // unbounded container does not discard the project's database data.
        run_docker(
            config,
            ["rm".to_owned(), "--force".to_owned(), cluster_name.clone()],
        )
        .await?;
    }

    if existing_state.is_none() || recreate_existing {
        let publish = format!("{}::5432", config.database_cluster_bind_address);
        let health_command = format!("pg_isready -U {} -d {}", spec.role_name, spec.database_name);
        let mut docker_args = vec![
            "run".to_owned(),
            "--detach".to_owned(),
            "--name".to_owned(),
            cluster_name.clone(),
            "--label".to_owned(),
            "com.knotree.managed-by=knotree-api".to_owned(),
            "--label".to_owned(),
            format!("com.knotree.project-id={}", spec.project_id),
            "--label".to_owned(),
            "com.knotree.resource-type=postgres".to_owned(),
            "--restart".to_owned(),
            "unless-stopped".to_owned(),
            "--network".to_owned(),
            network_name.clone(),
            "--network-alias".to_owned(),
            PROJECT_NETWORK_POSTGRES_ALIAS.to_owned(),
            "--env".to_owned(),
            format!("POSTGRES_DB={}", spec.database_name),
            "--env".to_owned(),
            format!("POSTGRES_USER={}", spec.role_name),
            "--env".to_owned(),
            format!("POSTGRES_PASSWORD={}", spec.password),
            "--env".to_owned(),
            "PGDATA=/var/lib/postgresql/data/pgdata".to_owned(),
            "--health-cmd".to_owned(),
            health_command,
            "--health-interval".to_owned(),
            "2s".to_owned(),
            "--health-timeout".to_owned(),
            "5s".to_owned(),
            "--health-retries".to_owned(),
            "30".to_owned(),
            "--publish".to_owned(),
            publish,
            "--volume".to_owned(),
            format!("{volume_name}:/var/lib/postgresql/data"),
        ];
        docker_args.extend(limits::docker_resource_limit_args());
        docker_args.push(config.database_cluster_image.clone());
        run_docker(config, docker_args).await?;
    } else {
        if existing_state.as_deref() != Some("running") {
            run_docker(config, ["start".to_owned(), cluster_name.clone()]).await?;
        }
        ensure_docker_runtime_limits(config, &cluster_name).await?;
    }

    ensure_docker_network_attachment(
        config,
        &network_name,
        &cluster_name,
        PROJECT_NETWORK_POSTGRES_ALIAS,
    )
    .await?;

    let port = docker_port(config, &cluster_name).await?;
    wait_for_postgres(
        &internal_host,
        port,
        &spec.database_name,
        &spec.role_name,
        &spec.password,
        config.database_cluster_startup_timeout_seconds,
    )
    .await?;

    let public_host = config
        .database_resource_public_host
        .clone()
        .map(|host| connection_host(PROVIDER_DOCKER, &host))
        .or_else(|| Some(internal_host.clone()));
    let public_port = config.database_resource_public_port.or(Some(port));

    Ok(ProvisionedCluster {
        provider: PROVIDER_DOCKER.to_owned(),
        name: cluster_name,
        namespace: None,
        volume: Some(volume_name),
        internal_host,
        internal_port: port,
        public_host,
        public_port,
        network_name: Some(network_name),
    })
}

pub async fn ensure_project_network(config: &Config, project_id: Uuid) -> Result<String> {
    let network_name = project_network_name(project_id);
    let inspect = docker_raw(
        config,
        [
            "network".to_owned(),
            "inspect".to_owned(),
            network_name.clone(),
        ],
    )
    .await?;
    if inspect.status.success() {
        return Ok(network_name);
    }

    let create = docker_raw(
        config,
        [
            "network".to_owned(),
            "create".to_owned(),
            "--driver".to_owned(),
            "bridge".to_owned(),
            "--label".to_owned(),
            "com.knotree.managed-by=knotree-api".to_owned(),
            "--label".to_owned(),
            format!("com.knotree.project-id={project_id}"),
            network_name.clone(),
        ],
    )
    .await?;
    if create.status.success() {
        return Ok(network_name);
    }

    // A concurrent resource provision may have created the deterministic
    // network between the inspect and create calls. Treat that race as safe.
    let retry_inspect = docker_raw(
        config,
        [
            "network".to_owned(),
            "inspect".to_owned(),
            network_name.clone(),
        ],
    )
    .await?;
    if retry_inspect.status.success() {
        return Ok(network_name);
    }

    let detail = String::from_utf8_lossy(&create.stderr);
    let detail = detail
        .lines()
        .next()
        .unwrap_or("unknown Docker network error");
    bail!("Docker project network operation failed: {detail}")
}

pub async fn ensure_docker_network_attachment(
    config: &Config,
    network_name: &str,
    container_name: &str,
    network_alias: &str,
) -> Result<()> {
    let output = docker_raw(
        config,
        [
            "inspect".to_owned(),
            "--format={{json .NetworkSettings.Networks}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.lines().next().unwrap_or("container not found");
        bail!("could not inspect Docker network attachment: {detail}");
    }

    let networks: Value = serde_json::from_slice(&output.stdout)
        .context("Docker returned invalid network attachment data")?;
    if network_has_alias(&networks, network_name, network_alias) {
        return Ok(());
    }

    if networks.get(network_name).is_some() {
        run_docker(
            config,
            [
                "network".to_owned(),
                "disconnect".to_owned(),
                "--force".to_owned(),
                network_name.to_owned(),
                container_name.to_owned(),
            ],
        )
        .await?;
    }

    let connect = docker_raw(
        config,
        [
            "network".to_owned(),
            "connect".to_owned(),
            "--alias".to_owned(),
            network_alias.to_owned(),
            network_name.to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if connect.status.success() {
        return Ok(());
    }

    let retry = docker_raw(
        config,
        [
            "inspect".to_owned(),
            "--format={{json .NetworkSettings.Networks}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if retry.status.success() {
        let networks: Value = serde_json::from_slice(&retry.stdout)
            .context("Docker returned invalid network attachment data")?;
        if network_has_alias(&networks, network_name, network_alias) {
            return Ok(());
        }
    }

    let detail = String::from_utf8_lossy(&connect.stderr);
    let detail = detail
        .lines()
        .next()
        .unwrap_or("unknown Docker network connection error");
    bail!("Docker project network connection failed: {detail}")
}

fn network_has_alias(networks: &Value, network_name: &str, network_alias: &str) -> bool {
    networks
        .get(network_name)
        .and_then(|network| network.get("Aliases"))
        .and_then(Value::as_array)
        .is_some_and(|aliases| {
            aliases
                .iter()
                .any(|alias| alias.as_str() == Some(network_alias))
        })
}

async fn ensure_docker_volume(config: &Config, volume_name: &str) -> Result<()> {
    let output = docker_raw(
        config,
        [
            "volume".to_owned(),
            "inspect".to_owned(),
            volume_name.to_owned(),
        ],
    )
    .await?;
    if output.status.success() {
        return Ok(());
    }
    run_docker(
        config,
        [
            "volume".to_owned(),
            "create".to_owned(),
            volume_name.to_owned(),
        ],
    )
    .await
    .map(|_| ())
}

async fn docker_inspect(config: &Config, cluster_name: &str) -> Result<Option<String>> {
    docker_container_status(config, cluster_name).await
}

async fn docker_container_status(config: &Config, container_name: &str) -> Result<Option<String>> {
    let output = docker_raw(
        config,
        [
            "inspect".to_owned(),
            "--format={{.State.Status}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if !output.status.success() {
        return Ok(None);
    }
    Ok(Some(
        String::from_utf8_lossy(&output.stdout).trim().to_owned(),
    ))
}

async fn docker_storage_limit_configured(config: &Config, container_name: &str) -> Result<bool> {
    let output = docker_raw(
        config,
        [
            "inspect".to_owned(),
            "--format={{json .HostConfig}}".to_owned(),
            container_name.to_owned(),
        ],
    )
    .await?;
    if !output.status.success() {
        return Ok(false);
    }
    let host_config = serde_json::from_slice::<Value>(&output.stdout).unwrap_or(Value::Null);
    Ok(host_config
        .get("StorageOpt")
        .or_else(|| host_config.get("StorageOpts"))
        .and_then(|storage_options| storage_options.get("size"))
        .and_then(Value::as_str)
        .is_some_and(|size| size.eq_ignore_ascii_case(RESOURCE_VOLUME_LIMIT_DOCKER)))
}

pub async fn docker_port(config: &Config, cluster_name: &str) -> Result<u16> {
    docker_published_port(config, cluster_name, 5432).await
}

pub async fn docker_published_port(
    config: &Config,
    cluster_name: &str,
    container_port: u16,
) -> Result<u16> {
    let output = run_docker(
        config,
        [
            "port".to_owned(),
            cluster_name.to_owned(),
            format!("{container_port}/tcp"),
        ],
    )
    .await?;
    output
        .lines()
        .filter_map(|line| line.rsplit(':').next())
        .find_map(|value| value.trim().parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("Docker did not publish port {container_port}"))
}

async fn run_docker<I>(config: &Config, args: I) -> Result<String>
where
    I: IntoIterator<Item = String>,
{
    let output = docker_raw(config, args).await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.lines().next().unwrap_or("unknown Docker error");
        bail!("Docker resource operation failed: {detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn docker_raw<I>(config: &Config, args: I) -> Result<std::process::Output>
where
    I: IntoIterator<Item = String>,
{
    Command::new(&config.database_cluster_docker_binary)
        .args(args)
        .output()
        .await
        .with_context(|| {
            format!(
                "could not execute resource provider binary `{}`",
                config.database_cluster_docker_binary
            )
        })
}

async fn wait_for_postgres(
    host: &str,
    port: u16,
    database_name: &str,
    role_name: &str,
    password: &str,
    timeout_seconds: u32,
) -> Result<()> {
    let options = PgConnectOptions::new()
        .host(host)
        .port(port)
        .database(database_name)
        .username(role_name)
        .password(password);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(u64::from(timeout_seconds));

    loop {
        if let Ok(Ok(mut connection)) =
            tokio::time::timeout(Duration::from_secs(3), PgConnection::connect_with(&options)).await
        {
            sqlx::query("SELECT 1")
                .execute(&mut connection)
                .await
                .context("created PostgreSQL cluster failed its readiness query")?;
            connection.close().await?;
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            bail!("PostgreSQL cluster did not become ready before the startup timeout");
        }
        sleep(Duration::from_millis(500)).await;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_running_docker_states_are_treated_as_running() {
        assert!(super::is_running_docker_state("running"));
        assert!(super::is_running_docker_state("RUNNING\n"));
        assert!(!super::is_running_docker_state("restarting"));
        assert!(!super::is_running_docker_state("true"));
    }

    #[test]
    fn cluster_names_are_stable_and_dns_safe() {
        let project_id = uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let name = format!("knotree-pg-{}", project_id.simple());
        assert_eq!(name, "knotree-pg-11111111222233334444555555555555");
        assert!(name.len() <= 63);
        assert!(name.chars().all(|character| character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'));
    }

    #[test]
    fn project_network_names_and_aliases_are_stable() {
        let project_id = uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        assert_eq!(
            super::project_network_name(project_id),
            "knotree-net-11111111222233334444555555555555"
        );
        let networks = serde_json::json!({
            "knotree-net-11111111222233334444555555555555": {
                "Aliases": ["postgres", "knotree-pg"]
            }
        });
        assert!(super::network_has_alias(
            &networks,
            "knotree-net-11111111222233334444555555555555",
            "postgres"
        ));
        assert!(!super::network_has_alias(
            &networks,
            "knotree-net-11111111222233334444555555555555",
            "app"
        ));
    }

    #[test]
    fn normalizes_localhost_for_docker_port_forwarding() {
        assert_eq!(
            super::connection_host(super::PROVIDER_DOCKER, "localhost"),
            "127.0.0.1"
        );
        assert_eq!(
            super::connection_host(super::PROVIDER_DOCKER, "db.internal"),
            "db.internal"
        );
        assert_eq!(
            super::connection_host(super::PROVIDER_KUBERNETES, "localhost"),
            "localhost"
        );
    }

    #[test]
    fn parses_docker_runtime_metrics() {
        let metrics = super::parse_docker_stats(
            r#"{"CPUPerc":"2.50%","MemUsage":"10.5MiB / 1GiB","NetIO":"1.5kB / 2MiB","BlockIO":"3MB / 4GiB"}"#,
        )
        .unwrap();

        assert_eq!(metrics.cpu_percent, Some(2.5));
        assert_eq!(metrics.memory_used_bytes, Some(11_010_048));
        assert_eq!(metrics.memory_limit_bytes, Some(1_073_741_824));
        assert_eq!(metrics.network_receive_bytes, Some(1_500));
        assert_eq!(metrics.network_transmit_bytes, Some(2_097_152));
        assert_eq!(metrics.disk_read_bytes, Some(3_000_000));
        assert_eq!(metrics.disk_write_bytes, Some(4_294_967_296));
    }

    #[test]
    fn parses_docker_filesystem_metrics() {
        let metrics = super::parse_docker_df(
            "Filesystem 1024-blocks Used Available Capacity Mounted on\noverlay 10485760 2048 10483712 1% /var/lib/postgresql/data\n",
        )
        .unwrap();

        assert_eq!(metrics, (2_097_152, 10_737_418_240));
        assert_eq!(
            super::parse_docker_du("32768\t/var/lib/postgresql/data\n").unwrap(),
            33_554_432
        );
    }

    #[test]
    fn applies_the_same_docker_limits_to_every_resource() {
        assert_eq!(
            super::docker_resource_limit_args(),
            vec![
                "--cpus",
                "1",
                "--memory",
                "1g",
                "--memory-swap",
                "1g",
                "--storage-opt",
                "size=10G",
            ]
        );
        assert_eq!(
            super::RESOURCE_VOLUME_LIMIT_BYTES,
            10 * 1024 * 1024 * 1024
        );
        assert_eq!(crate::limits::TENANT_RESOURCE_CAPS.cpu, "1");
    }
}
