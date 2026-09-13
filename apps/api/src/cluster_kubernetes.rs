use std::time::Duration;

use anyhow::{Context, Result, bail};
use kube::{
    Client,
    api::{Api, ApiResource, DynamicObject, GroupVersionKind, Patch, PatchParams},
};
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::{
    cluster::{ClusterSpec, PROVIDER_KUBERNETES, ProvisionedCluster},
    config::Config,
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
    wait_for_stateful_set(
        &stateful_sets,
        &name,
        config.database_cluster_startup_timeout_seconds,
    )
    .await?;

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
                        "resources": {
                            "requests": { "cpu": "100m", "memory": "256Mi" },
                            "limits": { "cpu": "1", "memory": "1Gi" },
                        },
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
                    "resources": { "requests": { "storage": config.database_cluster_storage_size } },
                },
            }],
        },
    })
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
        if tokio::time::Instant::now() >= deadline {
            bail!(
                "Kubernetes PostgreSQL StatefulSet did not become ready before the startup timeout"
            );
        }
        sleep(Duration::from_secs(2)).await;
    }
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
        let config = Config {
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
            app_service_provisioning_enabled: false,
            app_service_public_host: "localhost".to_owned(),
            app_service_bind_address: "127.0.0.1".parse().unwrap(),
            github_client_id: None,
            github_client_secret: None,
            github_oauth_redirect_uri: "http://localhost:8080/api/v1/auth/github/callback"
                .to_owned(),
            database_credentials_encryption_key: [7; 32],
        };
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
    }
}
