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
pub struct AppServiceDeploymentResponse {
    pub id: Uuid,
    pub status: String,
    pub current_step: String,
    pub logs: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppServiceLogsResponse {
    pub app_service_id: Uuid,
    pub container_name: Option<String>,
    pub status: String,
    pub running: bool,
    pub lines: Vec<String>,
    pub message: Option<String>,
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
    pub public_domain: Option<String>,
    pub public_access_enabled: bool,
    pub rate_limit_rpm: u32,
    pub container_name: Option<String>,
    pub error_message: Option<String>,
    pub auto_deploy_enabled: bool,
    pub deployed_image_digest: Option<String>,
    pub auto_deploy_checked_at: Option<String>,
    pub auto_deploy_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_connection: Option<AppServiceDatabaseConnectionResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployment: Option<AppServiceDeploymentResponse>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAppServiceRequest {
    pub name: Option<String>,
    pub image: String,
    pub image_source: String,
    pub app_port: Option<u32>,
    pub auto_deploy: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppServiceRequest {
    pub app_port: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppServicePublicAccessRequest {
    pub enabled: bool,
    pub rate_limit_rpm: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedisResourceResponse {
    pub id: Uuid,
    pub name: String,
    pub resource_type: String,
    pub status: String,
    pub host: String,
    pub port: u16,
    pub connection_string: Option<String>,
    pub cluster_provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub network_alias: String,
    pub cpu_limit: String,
    pub memory_limit: String,
    pub storage_limit: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDeploymentLog {
    pub app_service_id: Uuid,
    pub project_id: Uuid,
    pub status: String,
    pub current_step: String,
    pub logs: Vec<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppServiceAutoDeployRequest {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAppServiceDatabaseRequest {
    pub database_resource_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct CsrfResponse {
    #[serde(rename = "csrfToken")]
    pub csrf_token: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMetricPoint {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_network_receive_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_network_transmit_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_error_rate: Option<f64>,
}

pub type DatabaseMetricPoint = ResourceMetricPoint;
pub type AppServiceMetricPoint = ResourceMetricPoint;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceMetricsResponse {
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

pub type DatabaseMetricsResponse = ResourceMetricsResponse;
pub type AppServiceMetricsResponse = ResourceMetricsResponse;
