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

#[derive(Debug, Serialize)]
pub struct CsrfResponse {
    #[serde(rename = "csrfToken")]
    pub csrf_token: String,
}
