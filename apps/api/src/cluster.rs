use std::time::Duration;

use anyhow::{Context, Result, bail};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use tokio::{process::Command, time::sleep};
use uuid::Uuid;

use crate::config::Config;

pub const PROVIDER_DOCKER: &str = "docker";
pub const PROVIDER_KUBERNETES: &str = "kubernetes";

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

async fn provision_docker(config: &Config, spec: &ClusterSpec) -> Result<ProvisionedCluster> {
    let cluster_name = format!("knotree-pg-{}", spec.project_id.simple());
    let volume_name = format!("knotree-pg-data-{}", spec.project_id.simple());

    ensure_docker_volume(config, &volume_name).await?;

    let existing_state = docker_inspect(config, &cluster_name).await?;
    if existing_state.is_none() {
        let publish = format!("{}::5432", config.database_cluster_bind_address);
        let health_command = format!("pg_isready -U {} -d {}", spec.role_name, spec.database_name);
        run_docker(
            config,
            [
                "run".to_owned(),
                "--detach".to_owned(),
                "--name".to_owned(),
                cluster_name.clone(),
                "--label".to_owned(),
                "com.knotree.managed-by=knotree-api".to_owned(),
                "--label".to_owned(),
                format!("com.knotree.project-id={}", spec.project_id),
                "--restart".to_owned(),
                "unless-stopped".to_owned(),
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
                config.database_cluster_image.clone(),
            ],
        )
        .await?;
    } else if existing_state.as_deref() != Some("true") {
        run_docker(config, ["start".to_owned(), cluster_name.clone()]).await?;
    }

    let port = docker_port(config, &cluster_name).await?;
    wait_for_postgres(
        &config.database_resource_host,
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
        .or_else(|| Some(config.database_resource_host.clone()));
    let public_port = config.database_resource_public_port.or(Some(port));

    Ok(ProvisionedCluster {
        provider: PROVIDER_DOCKER.to_owned(),
        name: cluster_name,
        namespace: None,
        volume: Some(volume_name),
        internal_host: config.database_resource_host.clone(),
        internal_port: port,
        public_host,
        public_port,
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
    let output = docker_raw(
        config,
        [
            "inspect".to_owned(),
            "--format={{.State.Running}}".to_owned(),
            cluster_name.to_owned(),
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

async fn docker_port(config: &Config, cluster_name: &str) -> Result<u16> {
    let output = run_docker(
        config,
        [
            "port".to_owned(),
            cluster_name.to_owned(),
            "5432/tcp".to_owned(),
        ],
    )
    .await?;
    output
        .lines()
        .filter_map(|line| line.rsplit(':').next())
        .find_map(|value| value.trim().parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("Docker did not publish a PostgreSQL port"))
}

async fn run_docker<const N: usize>(config: &Config, args: [String; N]) -> Result<String> {
    let output = docker_raw(config, args).await?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.lines().next().unwrap_or("unknown Docker error");
        bail!("Docker cluster operation failed: {detail}");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

async fn docker_raw<const N: usize>(
    config: &Config,
    args: [String; N],
) -> Result<std::process::Output> {
    Command::new(&config.database_cluster_docker_binary)
        .args(args)
        .output()
        .await
        .with_context(|| {
            format!(
                "could not execute database cluster provider binary `{}`",
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
    fn cluster_names_are_stable_and_dns_safe() {
        let project_id = uuid::Uuid::parse_str("11111111-2222-3333-4444-555555555555").unwrap();
        let name = format!("knotree-pg-{}", project_id.simple());
        assert_eq!(name, "knotree-pg-11111111222233334444555555555555");
        assert!(name.len() <= 63);
        assert!(name.chars().all(|character| character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '-'));
    }
}
