use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
pub struct UserResponse {
    pub id: Uuid,
    #[serde(rename = "fullName")]
    pub full_name: String,
    pub email: String,
    #[serde(rename = "emailVerified")]
    pub email_verified: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectResponse {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthResponse {
    pub user: UserResponse,
    pub workspace: Option<WorkspaceResponse>,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    #[serde(rename = "fullName")]
    pub full_name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub name: String,
    pub slug: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub slug: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostgresResourceResponse {
    pub id: Uuid,
    pub name: String,
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub status: String,
    #[serde(rename = "databaseName")]
    pub database_name: String,
    pub username: String,
    pub host: String,
    pub port: u16,
    #[serde(rename = "connectionString")]
    pub connection_string: Option<String>,
    #[serde(rename = "clusterProvider")]
    pub cluster_provider: String,
    #[serde(rename = "clusterName", skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    #[serde(rename = "errorMessage", skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateResourceRequest {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppServiceDatabaseConnectionResponse {
    pub resource_id: Uuid,
    pub name: String,
    pub database_name: String,
    pub username: String,
    pub network_name: String,
    pub host: String,
    pub port: u16,
    pub environment_variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppServiceResponse {
    pub id: Uuid,
    pub name: String,
    pub resource_type: String,
    pub status: String,
    pub image: String,
    pub image_source: String,
    pub app_port: u16,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub service_url: Option<String>,
    pub container_name: Option<String>,
    pub error_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_connection: Option<AppServiceDatabaseConnectionResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAppServiceRequest {
    pub name: Option<String>,
    pub image: String,
    pub image_source: String,
    pub app_port: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct CsrfResponse {
    #[serde(rename = "csrfToken")]
    pub csrf_token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseMetricPoint {
    pub timestamp: i64,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseMetricsResponse {
    pub provider: String,
    pub system_metrics_available: bool,
    pub system_metrics_message: Option<String>,
    pub sample_interval_seconds: u32,
    pub retention_seconds: u32,
    pub range: String,
    pub from_timestamp: i64,
    pub to_timestamp: i64,
    pub resolution_seconds: u32,
    pub points: Vec<DatabaseMetricPoint>,
}
