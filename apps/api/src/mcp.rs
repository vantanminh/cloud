use std::{
    fs,
    path::{Path, PathBuf},
};

use axum::{
    Json,
    body::to_bytes,
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::{
    app_services, auth,
    error::AppError,
    security,
    state::AppState,
};

const AUTH_CODE_TTL_SECONDS: i64 = 600;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedTokenPair {
    pub access_token: String,
    pub refresh_token: String,
    pub access_token_hash: Vec<u8>,
    pub refresh_token_hash: Vec<u8>,
    pub access_expires_at: OffsetDateTime,
    pub refresh_expires_at: OffsetDateTime,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub redirect_uri: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub code_verifier: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub expires_in: i64,
    pub refresh_token: String,
    pub scope: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct ClientRegistrationRequest {
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
    pub token_endpoint_auth_method: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClientRegistrationResponse {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uris: Vec<String>,
    pub grant_types: Vec<&'static str>,
    pub response_types: Vec<&'static str>,
    pub token_endpoint_auth_method: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DocHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
}

pub fn issue_token_pair(
    now: OffsetDateTime,
    access_ttl_seconds: i64,
    refresh_ttl_seconds: i64,
) -> IssuedTokenPair {
    let access_ttl = Duration::seconds(access_ttl_seconds.max(1));
    let refresh_ttl = Duration::seconds(refresh_ttl_seconds.max(access_ttl_seconds.max(1)));
    let access_token = security::random_token();
    let refresh_token = security::random_token();
    IssuedTokenPair {
        access_token_hash: security::token_hash(&access_token),
        refresh_token_hash: security::token_hash(&refresh_token),
        access_token,
        refresh_token,
        access_expires_at: now + access_ttl,
        refresh_expires_at: now + refresh_ttl,
        expires_in: access_ttl.whole_seconds(),
    }
}

pub fn verify_pkce(verifier: &str, challenge: &str) -> bool {
    let digest = Sha256::digest(verifier.as_bytes());
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    encoded.as_bytes().ct_eq_slice(challenge.as_bytes())
}

trait ConstantEq {
    fn ct_eq_slice(&self, other: &[u8]) -> bool;
}

impl ConstantEq for [u8] {
    fn ct_eq_slice(&self, other: &[u8]) -> bool {
        use subtle::ConstantTimeEq;
        self.len() == other.len() && bool::from(self.ct_eq(other))
    }
}

pub fn access_token_is_valid(
    stored_hash: &[u8],
    expires_at: OffsetDateTime,
    presented: &str,
    now: OffsetDateTime,
    revoked: bool,
) -> bool {
    if revoked || now >= expires_at {
        return false;
    }
    security::token_hash(presented).as_slice().ct_eq_slice(stored_hash)
}

pub fn authorize_account(token_user: Uuid, resource_owner: Uuid) -> Result<(), AppError> {
    if token_user == resource_owner {
        Ok(())
    } else {
        Err(AppError::Forbidden {
            code: "MCP_FOREIGN_ACCOUNT",
            message: "This MCP token cannot access another account's resources.",
        })
    }
}

pub fn oauth_metadata(issuer: &str) -> Value {
    let issuer = issuer.trim_end_matches('/');
    json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/oauth/authorize"),
        "token_endpoint": format!("{issuer}/oauth/token"),
        "registration_endpoint": format!("{issuer}/oauth/register"),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["client_secret_post", "none"],
        "revocation_endpoint_auth_methods_supported": ["client_secret_post"],
        "scopes_supported": ["knotree"],
    })
}

pub fn protected_resource_metadata(issuer: &str) -> Value {
    let issuer = issuer.trim_end_matches('/');
    json!({
        "resource": format!("{issuer}/mcp"),
        "authorization_servers": [issuer],
        "bearer_methods_supported": ["header"],
        "scopes_supported": ["knotree"],
    })
}

pub fn search_docs(root: &Path, query: &str, limit: usize) -> Vec<DocHit> {
    let needle = query.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for path in markdown_files(root) {
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let Ok(body) = fs::read_to_string(&path) else {
            continue;
        };
        let haystack = format!(
            "{} {}",
            relative.display(),
            body
        )
        .to_ascii_lowercase();
        if !haystack.contains(&needle) {
            continue;
        }
        let title = body
            .lines()
            .find_map(|line| line.strip_prefix("# ").map(str::trim))
            .unwrap_or("Untitled")
            .to_owned();
        let snippet = body
            .lines()
            .find(|line| {
                !line.trim().is_empty()
                    && !line.starts_with('#')
                    && line.to_ascii_lowercase().contains(&needle)
            })
            .or_else(|| body.lines().find(|line| !line.trim().is_empty()))
            .unwrap_or("")
            .trim()
            .chars()
            .take(240)
            .collect();
        hits.push(DocHit {
            path: relative.to_string_lossy().replace('\\', "/"),
            title,
            snippet,
        });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

pub fn read_doc(root: &Path, relative: &str) -> Result<String, AppError> {
    let requested = PathBuf::from(relative.replace('\\', "/"));
    if requested.is_absolute()
        || requested
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(AppError::BadRequest {
            code: "MCP_DOC_PATH_INVALID",
            message: "Document paths must be relative to the project docs root.",
        });
    }
    let path = root.join(&requested);
    let Ok(canonical_root) = fs::canonicalize(root) else {
        return Err(AppError::NotFound {
            code: "MCP_DOC_NOT_FOUND",
            message: "The requested document could not be found.",
        });
    };
    let Ok(canonical) = fs::canonicalize(&path) else {
        return Err(AppError::NotFound {
            code: "MCP_DOC_NOT_FOUND",
            message: "The requested document could not be found.",
        });
    };
    if !canonical.starts_with(&canonical_root) {
        return Err(AppError::Forbidden {
            code: "MCP_DOC_PATH_INVALID",
            message: "Document paths must be relative to the project docs root.",
        });
    }
    fs::read_to_string(&canonical).map_err(|_| AppError::NotFound {
        code: "MCP_DOC_NOT_FOUND",
        message: "The requested document could not be found.",
    })
}

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "md" || ext == "MD")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

pub fn tool_definitions() -> Value {
    json!([
        {
            "name": "docs_search",
            "description": "Search Knotree Cloud product and operations docs to help deploy and debug app services.",
            "inputSchema": {
                "type": "object",
                "properties": { "query": { "type": "string" } },
                "required": ["query"]
            }
        },
        {
            "name": "docs_read",
            "description": "Read a Knotree Cloud documentation file by relative path.",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }
        },
        {
            "name": "account_logs",
            "description": "Read recent deployment logs across the authorized user's workspaces.",
            "inputSchema": { "type": "object", "properties": {} }
        },
        {
            "name": "service_logs",
            "description": "Read runtime logs for one app service the authorized user owns.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceSlug": { "type": "string" },
                    "projectSlug": { "type": "string" },
                    "appServiceId": { "type": "string" }
                },
                "required": ["workspaceSlug", "projectSlug", "appServiceId"]
            }
        },
        {
            "name": "list_resources",
            "description": "List PostgreSQL, Redis, and App services for a project the user owns.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceSlug": { "type": "string" },
                    "projectSlug": { "type": "string" }
                },
                "required": ["workspaceSlug", "projectSlug"]
            }
        },
        {
            "name": "create_redis",
            "description": "Provision a Redis instance on the project private network.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceSlug": { "type": "string" },
                    "projectSlug": { "type": "string" },
                    "name": { "type": "string" }
                },
                "required": ["workspaceSlug", "projectSlug"]
            }
        },
        {
            "name": "deploy_app_service",
            "description": "Deploy an App service from a container image or host an HTML page (paste index.html or a GitHub Pages-style repo with a unique page- hostname).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceSlug": { "type": "string" },
                    "projectSlug": { "type": "string" },
                    "image": { "type": "string" },
                    "imageSource": { "type": "string" },
                    "name": { "type": "string" },
                    "appPort": { "type": "integer" },
                    "pageSlug": { "type": "string" },
                    "indexHtml": { "type": "string" },
                    "githubRepo": { "type": "string" },
                    "githubBranch": { "type": "string" },
                    "autoDeploy": { "type": "boolean" }
                },
                "required": ["workspaceSlug", "projectSlug"]
            }
        },
        {
            "name": "setup_public_access",
            "description": "Enable or disable the random *.knotree.org hostname and optional Kong rate limit for an App service.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspaceSlug": { "type": "string" },
                    "projectSlug": { "type": "string" },
                    "appServiceId": { "type": "string" },
                    "enabled": { "type": "boolean" },
                    "rateLimitRpm": { "type": "integer" }
                },
                "required": ["workspaceSlug", "projectSlug", "appServiceId", "enabled"]
            }
        }
    ])
}

pub fn handle_initialize(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "knotree-cloud", "version": "0.1.0" }
        })),
        error: None,
    }
}

pub fn handle_tools_list(id: Option<Value>) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: Some(json!({ "tools": tool_definitions() })),
        error: None,
    }
}

pub async fn well_known_authorization_server(
    State(state): State<AppState>,
) -> Json<Value> {
    Json(oauth_metadata(&state.config.mcp_public_base_url))
}

pub async fn well_known_protected_resource(State(state): State<AppState>) -> Json<Value> {
    Json(protected_resource_metadata(&state.config.mcp_public_base_url))
}

pub async fn register_client(
    State(state): State<AppState>,
    Json(input): Json<ClientRegistrationRequest>,
) -> Result<(StatusCode, Json<ClientRegistrationResponse>), AppError> {
    if input.redirect_uris.is_empty() {
        return Err(AppError::BadRequest {
            code: "MCP_REDIRECT_URI_REQUIRED",
            message: "Register at least one redirect URI.",
        });
    }
    let client_id = format!("mcp_{}", Uuid::new_v4().simple());
    let client_secret = security::random_token();
    let name = input
        .client_name
        .unwrap_or_else(|| "MCP client".to_owned());
    sqlx::query(
        "INSERT INTO mcp_oauth_clients (id, client_id, client_secret_hash, redirect_uris, name) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(&client_id)
    .bind(security::token_hash(&client_secret))
    .bind(&input.redirect_uris)
    .bind(&name)
    .execute(&state.db)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(ClientRegistrationResponse {
            client_id,
            client_secret,
            redirect_uris: input.redirect_uris,
            grant_types: vec!["authorization_code", "refresh_token"],
            response_types: vec!["code"],
            token_endpoint_auth_method: "client_secret_post",
        }),
    ))
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeQuery {
    pub client_id: String,
    pub redirect_uri: String,
    pub response_type: Option<String>,
    pub state: Option<String>,
    pub code_challenge: String,
    pub code_challenge_method: Option<String>,
}

pub async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Query(query): axum::extract::Query<AuthorizeQuery>,
) -> Result<Response, AppError> {
    if query.response_type.as_deref().unwrap_or("code") != "code" {
        return Err(AppError::BadRequest {
            code: "MCP_RESPONSE_TYPE",
            message: "Only the authorization code response type is supported.",
        });
    }
    if query
        .code_challenge_method
        .as_deref()
        .unwrap_or("S256")
        != "S256"
    {
        return Err(AppError::BadRequest {
            code: "MCP_PKCE_REQUIRED",
            message: "PKCE S256 is required.",
        });
    }
    let user = match auth::authenticate(&state, &headers).await {
        Ok(user) => user,
        Err(_) => {
            let return_to = headers
                .get(header::REFERER)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("/");
            let login = format!(
                "{}/login?returnTo={}",
                state.config.mcp_public_base_url.trim_end_matches('/'),
                urlencoding_path(return_to)
            );
            return Ok(Redirect::temporary(&login).into_response());
        }
    };
    let client = sqlx::query_scalar::<_, i64>(
        "SELECT 1 FROM mcp_oauth_clients WHERE client_id = $1 AND $2 = ANY(redirect_uris)",
    )
    .bind(&query.client_id)
    .bind(&query.redirect_uri)
    .fetch_optional(&state.db)
    .await?;
    if client.is_none() {
        return Err(AppError::BadRequest {
            code: "MCP_CLIENT_INVALID",
            message: "The MCP client or redirect URI is not registered.",
        });
    }
    Ok(Html(authorize_page(&query, user.id)).into_response())
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeForm {
    pub client_id: String,
    pub redirect_uri: String,
    pub state: Option<String>,
    pub code_challenge: String,
    pub decision: String,
}

pub async fn authorize_submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Form(form): axum::extract::Form<AuthorizeForm>,
) -> Result<Response, AppError> {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if let Some(origin) = origin {
        let issuer = state.config.mcp_public_base_url.trim_end_matches('/');
        if !state.config.is_allowed_origin(origin) && origin != issuer {
            return Err(AppError::Forbidden {
                code: "ORIGIN_NOT_ALLOWED",
                message: "The request origin is not allowed.",
            });
        }
    }
    let user = auth::authenticate(&state, &headers).await?;
    if form.decision != "approve" {
        return Ok(Redirect::temporary(&deny_redirect(&form)).into_response());
    }
    let code = security::random_token();
    sqlx::query(
        "INSERT INTO mcp_oauth_codes (code_hash, client_id, user_id, redirect_uri, code_challenge, expires_at) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(security::token_hash(&code))
    .bind(&form.client_id)
    .bind(user.id)
    .bind(&form.redirect_uri)
    .bind(&form.code_challenge)
    .bind(OffsetDateTime::now_utc() + Duration::seconds(AUTH_CODE_TTL_SECONDS))
    .execute(&state.db)
    .await?;
    let mut location = url::Url::parse(&form.redirect_uri).map_err(|_| AppError::BadRequest {
        code: "MCP_REDIRECT_URI_INVALID",
        message: "The redirect URI is invalid.",
    })?;
    location.query_pairs_mut().append_pair("code", &code);
    if let Some(state_value) = form.state.as_deref() {
        location.query_pairs_mut().append_pair("state", state_value);
    }
    Ok(Redirect::temporary(location.as_str()).into_response())
}

const MAX_TOKEN_BODY_BYTES: usize = 32 * 1024;

pub async fn token(
    State(state): State<AppState>,
    request: Request,
) -> Result<Json<TokenResponse>, AppError> {
    let input = parse_token_request(request).await?;
    match input.grant_type.as_str() {
        "authorization_code" => issue_from_code(&state, input).await,
        "refresh_token" => refresh_access_token(&state, input).await,
        _ => Err(AppError::BadRequest {
            code: "MCP_GRANT_UNSUPPORTED",
            message: "Use authorization_code or refresh_token.",
        }),
    }
}

pub async fn parse_token_request(request: Request) -> Result<TokenRequest, AppError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_owned();
    let bytes = to_bytes(request.into_body(), MAX_TOKEN_BODY_BYTES)
        .await
        .map_err(|_| AppError::BadRequest {
            code: "MCP_TOKEN_BODY_INVALID",
            message: "The token request body is invalid.",
        })?;
    parse_token_request_bytes(&content_type, &bytes)
}

pub fn parse_token_request_bytes(
    content_type: &str,
    body: &[u8],
) -> Result<TokenRequest, AppError> {
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim();
    if media_type.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
        serde_urlencoded::from_bytes(body).map_err(|_| AppError::BadRequest {
            code: "MCP_TOKEN_BODY_INVALID",
            message: "The token request body is invalid.",
        })
    } else {
        serde_json::from_slice(body).map_err(|_| AppError::BadRequest {
            code: "MCP_TOKEN_BODY_INVALID",
            message: "The token request body is invalid.",
        })
    }
}

async fn issue_from_code(
    state: &AppState,
    input: TokenRequest,
) -> Result<Json<TokenResponse>, AppError> {
    let code = input.code.ok_or(AppError::BadRequest {
        code: "MCP_CODE_REQUIRED",
        message: "The authorization code is required.",
    })?;
    let redirect_uri = input.redirect_uri.ok_or(AppError::BadRequest {
        code: "MCP_REDIRECT_URI_REQUIRED",
        message: "The redirect URI is required.",
    })?;
    let client_id = input.client_id.ok_or(AppError::BadRequest {
        code: "MCP_CLIENT_REQUIRED",
        message: "The client id is required.",
    })?;
    let verifier = input.code_verifier.ok_or(AppError::BadRequest {
        code: "MCP_PKCE_REQUIRED",
        message: "The PKCE code verifier is required.",
    })?;
    let row = sqlx::query_as::<_, CodeRow>(
        "SELECT user_id, redirect_uri, code_challenge, expires_at FROM mcp_oauth_codes WHERE code_hash = $1 AND client_id = $2",
    )
    .bind(security::token_hash(&code))
    .bind(&client_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized {
        code: "MCP_CODE_INVALID",
        message: "The authorization code is invalid.",
    })?;
    sqlx::query("DELETE FROM mcp_oauth_codes WHERE code_hash = $1")
        .bind(security::token_hash(&code))
        .execute(&state.db)
        .await?;
    if row.redirect_uri != redirect_uri || row.expires_at <= OffsetDateTime::now_utc() {
        return Err(AppError::Unauthorized {
            code: "MCP_CODE_INVALID",
            message: "The authorization code is invalid.",
        });
    }
    if !verify_pkce(&verifier, &row.code_challenge) {
        return Err(AppError::Unauthorized {
            code: "MCP_PKCE_INVALID",
            message: "The PKCE verifier does not match.",
        });
    }
    persist_token_pair(state, row.user_id, &client_id).await
}

async fn refresh_access_token(
    state: &AppState,
    input: TokenRequest,
) -> Result<Json<TokenResponse>, AppError> {
    let refresh = input.refresh_token.ok_or(AppError::BadRequest {
        code: "MCP_REFRESH_REQUIRED",
        message: "The refresh token is required.",
    })?;
    let row = sqlx::query_as::<_, TokenRow>(
        "SELECT id, user_id, client_id, refresh_expires_at, revoked_at FROM mcp_oauth_tokens WHERE refresh_token_hash = $1",
    )
    .bind(security::token_hash(&refresh))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized {
        code: "MCP_REFRESH_INVALID",
        message: "The refresh token is invalid.",
    })?;
    if row.revoked_at.is_some() || row.refresh_expires_at <= OffsetDateTime::now_utc() {
        return Err(AppError::Unauthorized {
            code: "MCP_REFRESH_INVALID",
            message: "The refresh token is invalid.",
        });
    }
    sqlx::query("UPDATE mcp_oauth_tokens SET revoked_at = now() WHERE id = $1")
        .bind(row.id)
        .execute(&state.db)
        .await?;
    persist_token_pair(state, row.user_id, &row.client_id).await
}

async fn persist_token_pair(
    state: &AppState,
    user_id: Uuid,
    client_id: &str,
) -> Result<Json<TokenResponse>, AppError> {
    let pair = issue_token_pair(
        OffsetDateTime::now_utc(),
        state.config.mcp_access_ttl_seconds,
        state.config.mcp_refresh_ttl_seconds,
    );
    sqlx::query(
        "INSERT INTO mcp_oauth_tokens (id, user_id, client_id, access_token_hash, refresh_token_hash, access_expires_at, refresh_expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::new_v4())
    .bind(user_id)
    .bind(client_id)
    .bind(&pair.access_token_hash)
    .bind(&pair.refresh_token_hash)
    .bind(pair.access_expires_at)
    .bind(pair.refresh_expires_at)
    .execute(&state.db)
    .await?;
    Ok(Json(TokenResponse {
        access_token: pair.access_token,
        token_type: "Bearer",
        expires_in: pair.expires_in,
        refresh_token: pair.refresh_token,
        scope: "knotree",
    }))
}

pub async fn mcp_endpoint(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, AppError> {
    let user_id = authenticate_mcp(&state, &headers).await?;
    Ok(Json(dispatch_mcp(&state, user_id, request).await))
}

async fn authenticate_mcp(state: &AppState, headers: &HeaderMap) -> Result<Uuid, AppError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthorized {
            code: "MCP_TOKEN_REQUIRED",
            message: "A Bearer access token is required.",
        })?;
    let row = sqlx::query_as::<_, AccessRow>(
        "SELECT user_id, access_token_hash, access_expires_at, revoked_at FROM mcp_oauth_tokens WHERE access_token_hash = $1",
    )
    .bind(security::token_hash(token))
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::Unauthorized {
        code: "MCP_TOKEN_INVALID",
        message: "The access token is invalid.",
    })?;
    if !access_token_is_valid(
        &row.access_token_hash,
        row.access_expires_at,
        token,
        OffsetDateTime::now_utc(),
        row.revoked_at.is_some(),
    ) {
        return Err(AppError::Unauthorized {
            code: "MCP_TOKEN_INVALID",
            message: "The access token is invalid.",
        });
    }
    Ok(row.user_id)
}

async fn dispatch_mcp(state: &AppState, user_id: Uuid, request: JsonRpcRequest) -> JsonRpcResponse {
    if request.jsonrpc != "2.0" {
        return rpc_error(request.id, -32600, "Invalid JSON-RPC version.");
    }
    match request.method.as_str() {
        "initialize" => handle_initialize(request.id),
        "notifications/initialized" => JsonRpcResponse {
            jsonrpc: "2.0",
            id: request.id,
            result: Some(json!({})),
            error: None,
        },
        "tools/list" => handle_tools_list(request.id),
        "tools/call" => match call_tool(state, user_id, request.params.unwrap_or(json!({}))).await {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0",
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(error) => rpc_app_error(request.id, error),
        },
        _ => rpc_error(request.id, -32601, "Method not found."),
    }
}

async fn call_tool(state: &AppState, user_id: Uuid, params: Value) -> Result<Value, AppError> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or(AppError::BadRequest {
            code: "MCP_TOOL_REQUIRED",
            message: "A tool name is required.",
        })?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let payload = match name {
        "docs_search" => {
            let query = arguments
                .get("query")
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({ "hits": search_docs(Path::new(&state.config.docs_dir), query, 12) })
        }
        "docs_read" => {
            let path = arguments
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            json!({ "path": path, "content": read_doc(Path::new(&state.config.docs_dir), path)? })
        }
        "setup_public_access" => app_services::mcp_setup_public_access(
            state,
            user_id,
            arguments
                .get("workspaceSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments
                .get("projectSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments
                .get("appServiceId")
                .and_then(Value::as_str)
                .and_then(|value| Uuid::parse_str(value).ok())
                .ok_or(AppError::BadRequest {
                    code: "MCP_SERVICE_ID_INVALID",
                    message: "A valid appServiceId is required.",
                })?,
            arguments
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            arguments
                .get("rateLimitRpm")
                .and_then(Value::as_u64)
                .map(|value| value as u32),
        )
        .await?,
        "account_logs" => json!({ "deployments": app_services::account_deployment_logs(state, user_id).await? }),
        "service_logs" => {
            let logs = app_services::mcp_service_logs(
                state,
                user_id,
                arguments
                    .get("workspaceSlug")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                arguments
                    .get("projectSlug")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                arguments
                    .get("appServiceId")
                    .and_then(Value::as_str)
                    .and_then(|value| Uuid::parse_str(value).ok())
                    .ok_or(AppError::BadRequest {
                        code: "MCP_SERVICE_ID_INVALID",
                        message: "A valid appServiceId is required.",
                    })?,
            )
            .await?;
            serde_json::to_value(logs).map_err(AppError::internal)?
        }
        "list_resources" => app_services::mcp_list_resources(
            state,
            user_id,
            arguments
                .get("workspaceSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments
                .get("projectSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
        .await?,
        "create_redis" => crate::redis_resources::mcp_create(
            state,
            user_id,
            arguments
                .get("workspaceSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments
                .get("projectSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments.get("name").and_then(Value::as_str),
        )
        .await?,
        "deploy_app_service" => app_services::mcp_deploy(
            state,
            user_id,
            arguments
                .get("workspaceSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments
                .get("projectSlug")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            arguments.clone(),
        )
        .await?,
        _ => {
            return Err(AppError::BadRequest {
                code: "MCP_TOOL_UNKNOWN",
                message: "Unknown MCP tool.",
            });
        }
    };
    Ok(json!({
        "content": [{ "type": "text", "text": payload.to_string() }],
        "structuredContent": payload
    }))
}

fn rpc_error(id: Option<Value>, code: i64, message: &str) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(json!({ "code": code, "message": message })),
    }
}

fn rpc_app_error(id: Option<Value>, error: AppError) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(json!({
            "code": -32000,
            "message": match &error {
                AppError::BadRequest { message, .. }
                | AppError::Unauthorized { message, .. }
                | AppError::Forbidden { message, .. }
                | AppError::NotFound { message, .. }
                | AppError::Conflict { message, .. }
                | AppError::ServiceUnavailable { message, .. }
                | AppError::TooManyRequests { message, .. } => (*message).to_owned(),
                AppError::Validation { .. } => "Validation failed.".to_owned(),
                AppError::Internal(_) => "Internal error.".to_owned(),
            }
        })),
    }
}

fn authorize_page(query: &AuthorizeQuery, user_id: Uuid) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Authorize MCP · Knotree</title>
<style>body{{font-family:Inter,system-ui,sans-serif;background:#0b1020;color:#f7f8ff;display:grid;place-items:center;min-height:100vh;margin:0}}main{{width:min(28rem,calc(100% - 3rem));padding:1.5rem;border:1px solid #26304e;border-radius:1rem;background:#121a31}}button{{margin-right:.5rem;padding:.6rem 1rem;border-radius:.6rem;border:0;font-weight:700;cursor:pointer}}.ok{{background:#91a7ff}}.no{{background:#26304e;color:#fff}}</style></head>
<body><main><p>Knotree MCP</p><h1>Allow this agent?</h1><p>Client <code>{}</code> can read docs, logs, and manage resources for user {}.</p>
<form method="post"><input type="hidden" name="client_id" value="{}"><input type="hidden" name="redirect_uri" value="{}"><input type="hidden" name="code_challenge" value="{}"><input type="hidden" name="state" value="{}"><button class="ok" name="decision" value="approve">Approve</button><button class="no" name="decision" value="deny">Deny</button></form></main></body></html>"#,
        escape(&query.client_id),
        user_id,
        escape(&query.client_id),
        escape(&query.redirect_uri),
        escape(&query.code_challenge),
        escape(query.state.as_deref().unwrap_or("")),
    )
}

fn deny_redirect(form: &AuthorizeForm) -> String {
    format!(
        "{}{}error=access_denied{}",
        form.redirect_uri,
        if form.redirect_uri.contains('?') {
            "&"
        } else {
            "?"
        },
        form.state
            .as_deref()
            .map(|state| format!("&state={state}"))
            .unwrap_or_default()
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('"', "&quot;")
}

fn urlencoding_path(value: &str) -> String {
    value.replace(' ', "%20")
}

#[derive(Debug, sqlx::FromRow)]
struct CodeRow {
    user_id: Uuid,
    redirect_uri: String,
    code_challenge: String,
    expires_at: OffsetDateTime,
}

#[derive(Debug, sqlx::FromRow)]
struct TokenRow {
    id: Uuid,
    user_id: Uuid,
    client_id: String,
    refresh_expires_at: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
}

#[derive(Debug, sqlx::FromRow)]
struct AccessRow {
    user_id: Uuid,
    access_token_hash: Vec<u8>,
    access_expires_at: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issues_access_and_refresh_tokens() {
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let pair = issue_token_pair(now, 3600, 86400);
        assert!(!pair.access_token.is_empty());
        assert!(!pair.refresh_token.is_empty());
        assert_ne!(pair.access_token, pair.refresh_token);
        assert!(access_token_is_valid(
            &pair.access_token_hash,
            pair.access_expires_at,
            &pair.access_token,
            now + Duration::seconds(10),
            false
        ));
        assert!(!access_token_is_valid(
            &pair.access_token_hash,
            pair.access_expires_at,
            "other",
            now + Duration::seconds(10),
            false
        ));
    }

    #[test]
    fn token_endpoint_accepts_form_urlencoded_and_json() {
        let form = parse_token_request_bytes(
            "application/x-www-form-urlencoded; charset=UTF-8",
            b"grant_type=refresh_token&refresh_token=rt-1&client_id=mcp_1",
        )
        .unwrap();
        assert_eq!(form.grant_type, "refresh_token");
        assert_eq!(form.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(form.client_id.as_deref(), Some("mcp_1"));

        let json = parse_token_request_bytes(
            "application/json",
            br#"{"grant_type":"authorization_code","code":"c","redirect_uri":"http://localhost/cb","client_id":"mcp_1","code_verifier":"v"}"#,
        )
        .unwrap();
        assert_eq!(json.grant_type, "authorization_code");
        assert_eq!(json.code.as_deref(), Some("c"));
        assert_eq!(json.code_verifier.as_deref(), Some("v"));
    }

    #[tokio::test]
    async fn refresh_yields_a_new_access_token() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let seed = crate::test_support::seed_owner_project(&state).await;
        let Json(original) = super::persist_token_pair(&state, seed.user_id, "mcp_refresh_client")
            .await
            .unwrap();
        let Json(refreshed) = super::refresh_access_token(
            &state,
            TokenRequest {
                grant_type: "refresh_token".to_owned(),
                code: None,
                redirect_uri: None,
                client_id: Some("mcp_refresh_client".to_owned()),
                client_secret: None,
                code_verifier: None,
                refresh_token: Some(original.refresh_token.clone()),
            },
        )
        .await
        .unwrap();
        assert_ne!(refreshed.access_token, original.access_token);
        assert_ne!(refreshed.refresh_token, original.refresh_token);

        let form = format!(
            "grant_type=refresh_token&refresh_token={}",
            refreshed.refresh_token
        );
        let request = axum::http::Request::builder()
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(axum::body::Body::from(form))
            .unwrap();
        let Json(from_form) = token(axum::extract::State(state.clone()), request)
            .await
            .unwrap();
        assert_ne!(from_form.access_token, refreshed.access_token);

        let reused = super::refresh_access_token(
            &state,
            TokenRequest {
                grant_type: "refresh_token".to_owned(),
                code: None,
                redirect_uri: None,
                client_id: None,
                client_secret: None,
                code_verifier: None,
                refresh_token: Some(original.refresh_token),
            },
        )
        .await;
        assert!(reused.is_err());
    }

    #[tokio::test]
    async fn tools_call_real_docs_deploy_logs_and_public_setup() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let seed = crate::test_support::seed_owner_project(&state).await;
        let context = |name: &str, arguments: Value| {
            json!({
                "name": name,
                "arguments": arguments,
            })
        };

        let docs = call_tool(
            &state,
            seed.user_id,
            context("docs_search", json!({ "query": "PostgreSQL" })),
        )
        .await
        .expect("docs tool");
        assert!(!docs["structuredContent"]["hits"]
            .as_array()
            .expect("doc hits")
            .is_empty());

        let deployed = call_tool(
            &state,
            seed.user_id,
            context(
                "deploy_app_service",
                json!({
                    "workspaceSlug": &seed.workspace_slug,
                    "projectSlug": &seed.project_slug,
                    "name": "MCP site",
                    "image": "nginx:alpine",
                    "imageSource": "public",
                    "appPort": 80,
                }),
            ),
        )
        .await
        .expect("deploy tool");
        let service_id = deployed["structuredContent"]["id"]
            .as_str()
            .and_then(|value| Uuid::parse_str(value).ok())
            .expect("deployed service id");

        let logs = call_tool(
            &state,
            seed.user_id,
            context(
                "service_logs",
                json!({
                    "workspaceSlug": &seed.workspace_slug,
                    "projectSlug": &seed.project_slug,
                    "appServiceId": service_id,
                }),
            ),
        )
        .await
        .expect("service logs tool");
        assert_eq!(logs["structuredContent"]["appServiceId"], service_id.to_string());

        let setup = call_tool(
            &state,
            seed.user_id,
            context(
                "setup_public_access",
                json!({
                    "workspaceSlug": &seed.workspace_slug,
                    "projectSlug": &seed.project_slug,
                    "appServiceId": service_id,
                    "enabled": true,
                    "rateLimitRpm": 90,
                }),
            ),
        )
        .await
        .expect("public access tool");
        assert_eq!(setup["structuredContent"]["publicAccessEnabled"], true);
        assert_eq!(setup["structuredContent"]["rateLimitRpm"], 90);
        assert!(setup["structuredContent"]["publicDomain"]
            .as_str()
            .is_some_and(|domain| domain.ends_with(".knotree.org")));
    }

    #[test]
    fn foreign_account_tool_calls_fail() {
        let owner = Uuid::nil();
        let other = Uuid::from_u128(7);
        assert!(authorize_account(owner, owner).is_ok());
        assert!(authorize_account(other, owner).is_err());
    }

    #[test]
    fn docs_search_and_read_use_the_project_docs_corpus() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs");
        let hits = search_docs(&root, "PostgreSQL", 5);
        assert!(!hits.is_empty());
        let content = read_doc(&root, &hits[0].path).unwrap();
        assert!(content.to_ascii_lowercase().contains("postgres") || content.contains("#"));
        assert!(read_doc(&root, "../Cargo.toml").is_err());
    }

    #[test]
    fn oauth_metadata_advertises_refresh_tokens() {
        let metadata = oauth_metadata("https://cloud.knotree.com");
        assert_eq!(metadata["issuer"], "https://cloud.knotree.com");
        assert!(
            metadata["grant_types_supported"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == "refresh_token")
        );
        assert_eq!(
            protected_resource_metadata("https://cloud.knotree.com")["resource"],
            "https://cloud.knotree.com/mcp"
        );
    }

    #[test]
    fn pkce_s256_matches_the_challenge() {
        let verifier = "test-verifier-value-that-is-long-enough";
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        assert!(verify_pkce(verifier, &challenge));
        assert!(!verify_pkce("other", &challenge));
    }

    #[test]
    fn tools_list_includes_docs_logs_and_setup() {
        let tools = tool_definitions();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert!(names.contains(&"docs_search"));
        assert!(names.contains(&"account_logs"));
        assert!(names.contains(&"service_logs"));
        assert!(names.contains(&"setup_public_access"));
        assert!(names.contains(&"deploy_app_service"));
        assert!(names.contains(&"create_redis"));
    }

    #[test]
    fn expired_or_revoked_tokens_are_rejected() {
        let now = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let pair = issue_token_pair(now, 30, 60);
        assert!(!access_token_is_valid(
            &pair.access_token_hash,
            pair.access_expires_at,
            &pair.access_token,
            pair.access_expires_at,
            false
        ));
        assert!(!access_token_is_valid(
            &pair.access_token_hash,
            pair.access_expires_at,
            &pair.access_token,
            now + Duration::seconds(31),
            false
        ));
        assert!(!access_token_is_valid(
            &pair.access_token_hash,
            pair.access_expires_at,
            &pair.access_token,
            now,
            true
        ));
    }
}
