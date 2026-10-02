use std::collections::{BTreeMap, BTreeSet};

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{auth, error::AppError, projects, security, state::AppState};

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_UPLOAD_BODY: usize = MAX_IMAGE_BYTES + 64 * 1024;
const MAX_STORE_BYTES: i64 = 1024 * 1024 * 1024;
const MAX_OBJECTS_PER_STORE: i64 = 10_000;
const MAX_STORES_PER_PROJECT: i64 = 20;
const MAX_KEYS_PER_STORE: i64 = 20;
const MAX_DIMENSION: u32 = 8_192;
const MAX_PIXELS: u64 = 16_000_000;
const PUBLIC_CACHE_SECONDS: u32 = 31_536_000;
const PRIVATE_CACHE_SECONDS: u32 = 60;
const PRIVATE_MAX_EXPIRY_SECONDS: i64 = 7 * 24 * 60 * 60;

pub const UPLOAD_BODY_LIMIT: usize = MAX_UPLOAD_BODY;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageStoreResponse {
    pub id: Uuid,
    pub name: String,
    pub resource_type: &'static str,
    pub compression_mode: String,
    pub max_width: Option<i32>,
    pub max_height: Option<i32>,
    pub quality: Option<i32>,
    pub object_count: i64,
    pub byte_size: i64,
    pub public_base_url: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageKeyResponse {
    pub id: Uuid,
    pub name: String,
    pub client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
    pub access: String,
    pub status: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageObjectResponse {
    pub id: Uuid,
    pub folder: String,
    pub file_name: String,
    pub content_type: String,
    pub byte_size: i64,
    pub width: i32,
    pub height: i32,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageListResponse {
    pub objects: Vec<ImageObjectResponse>,
    pub folders: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedImageResponse {
    pub url: String,
    pub visibility: String,
    pub expires_at: Option<String>,
    pub cache_seconds: u32,
    pub content_type: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedFolderResponse {
    pub deleted: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SaveStoreBody {
    name: String,
    compression_mode: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
    quality: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStoreBody {
    name: Option<String>,
    compression_mode: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
    quality: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CreateKeyBody {
    name: String,
    access: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SignBody {
    visibility: String,
    expires_in_seconds: Option<i64>,
    width: Option<u32>,
    height: Option<u32>,
    quality: Option<u32>,
    key_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ListParams {
    folder: Option<String>,
    #[serde(default)]
    recursive: bool,
}

#[derive(Debug, Deserialize)]
pub(crate) struct FolderParams {
    folder: String,
}

#[derive(Debug, sqlx::FromRow)]
struct StoreRow {
    id: Uuid,
    #[allow(dead_code)]
    project_id: Uuid,
    name: String,
    compression_mode: String,
    max_width: Option<i32>,
    max_height: Option<i32>,
    quality: Option<i32>,
}

#[derive(Debug, sqlx::FromRow)]
struct KeyRow {
    id: Uuid,
    store_id: Uuid,
    name: String,
    client_id: String,
    secret_hash: Vec<u8>,
    access: String,
    status: String,
    revoked_at: Option<OffsetDateTime>,
    created_at: OffsetDateTime,
}

#[derive(Debug, sqlx::FromRow)]
struct ObjectMeta {
    id: Uuid,
    folder: String,
    file_name: String,
    content_type: String,
    byte_size: i64,
    width: i32,
    height: i32,
    created_at: OffsetDateTime,
}

#[derive(Debug, sqlx::FromRow)]
struct ObjectBlob {
    content_type: String,
    data: Vec<u8>,
}

struct CompressionSettings {
    mode: String,
    max_width: Option<i32>,
    max_height: Option<i32>,
    quality: Option<i32>,
}

struct SignedVariant {
    visibility: String,
    mode: String,
    width: Option<u32>,
    height: Option<u32>,
    quality: Option<u32>,
    exp: Option<i64>,
    kid: Option<String>,
}

enum Permission {
    Upload,
    Delete,
    List,
    SignPublic,
    SignPrivate,
}

pub async fn list_stores(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug)): Path<(String, String)>,
) -> Result<Json<Vec<ImageStoreResponse>>, AppError> {
    let project_id = session_project(&state, &headers, &workspace_id, &project_slug, false).await?;
    let stores = sqlx::query_as::<_, StoreRow>(
        "SELECT id, project_id, name, compression_mode, max_width, max_height, quality
         FROM image_stores WHERE project_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?;
    let mut responses = Vec::with_capacity(stores.len());
    for store in stores {
        responses.push(store_response(&state, &store).await?);
    }
    Ok(Json(responses))
}

pub(crate) async fn create_store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug)): Path<(String, String)>,
    Json(body): Json<SaveStoreBody>,
) -> Result<(StatusCode, Json<ImageStoreResponse>), AppError> {
    let project_id = session_project(&state, &headers, &workspace_id, &project_slug, true).await?;
    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM image_stores WHERE project_id = $1",
    )
    .bind(project_id)
    .fetch_one(&state.db)
    .await?;
    if count >= MAX_STORES_PER_PROJECT {
        return Err(AppError::Conflict {
            code: "IMAGE_STORE_LIMIT",
            message: "This project already has the maximum number of image stores.",
        });
    }
    let name = normalize_name(&body.name)?;
    let settings = normalize_compression(
        &body.compression_mode,
        body.max_width,
        body.max_height,
        body.quality,
    )?;
    let id = Uuid::new_v4();
    let store = sqlx::query_as::<_, StoreRow>(
        "INSERT INTO image_stores (id, project_id, name, compression_mode, max_width, max_height, quality)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, project_id, name, compression_mode, max_width, max_height, quality",
    )
    .bind(id)
    .bind(project_id)
    .bind(name)
    .bind(settings.mode)
    .bind(settings.max_width)
    .bind(settings.max_height)
    .bind(settings.quality)
    .fetch_one(&state.db)
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(store_response(&state, &store).await?),
    ))
}

pub(crate) async fn update_store(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id)): Path<(String, String, Uuid)>,
    Json(body): Json<UpdateStoreBody>,
) -> Result<Json<ImageStoreResponse>, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    let name = match body.name {
        Some(name) => normalize_name(&name)?,
        None => store.name,
    };
    let settings = normalize_compression(
        &body.compression_mode,
        body.max_width,
        body.max_height,
        body.quality,
    )?;
    let store = sqlx::query_as::<_, StoreRow>(
        "UPDATE image_stores
         SET name = $2, compression_mode = $3, max_width = $4, max_height = $5, quality = $6, updated_at = now()
         WHERE id = $1
         RETURNING id, project_id, name, compression_mode, max_width, max_height, quality",
    )
    .bind(store.id)
    .bind(name)
    .bind(settings.mode)
    .bind(settings.max_width)
    .bind(settings.max_height)
    .bind(settings.quality)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(store_response(&state, &store).await?))
}

pub(crate) async fn create_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id)): Path<(String, String, Uuid)>,
    Json(body): Json<CreateKeyBody>,
) -> Result<(StatusCode, Json<ImageKeyResponse>), AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    let count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM image_api_keys WHERE store_id = $1",
    )
    .bind(store.id)
    .fetch_one(&state.db)
    .await?;
    if count >= MAX_KEYS_PER_STORE {
        return Err(AppError::Conflict {
            code: "IMAGE_KEY_LIMIT",
            message: "This image store already has the maximum number of client keys.",
        });
    }
    let name = normalize_name(&body.name)?;
    let access = normalize_access(&body.access)?;
    let client_id = format!("kimg_{}", security::random_token());
    let client_secret = format!("ksec_{}", security::random_token());
    let id = Uuid::new_v4();
    let key = sqlx::query_as::<_, KeyRow>(
        "INSERT INTO image_api_keys (id, store_id, name, client_id, secret_hash, access)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, store_id, name, client_id, secret_hash, access, status, revoked_at, created_at",
    )
    .bind(id)
    .bind(store.id)
    .bind(name)
    .bind(&client_id)
    .bind(security::token_hash(&client_secret))
    .bind(access)
    .fetch_one(&state.db)
    .await?;
    let mut response = key_response(&key);
    response.client_secret = Some(client_secret);
    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn list_keys(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id)): Path<(String, String, Uuid)>,
) -> Result<Json<Vec<ImageKeyResponse>>, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        false,
    )
    .await?;
    let keys = sqlx::query_as::<_, KeyRow>(
        "SELECT id, store_id, name, client_id, secret_hash, access, status, revoked_at, created_at
         FROM image_api_keys WHERE store_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(store.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(keys.iter().map(key_response).collect()))
}

pub async fn revoke_key(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id, key_id)): Path<(String, String, Uuid, Uuid)>,
) -> Result<Json<ImageKeyResponse>, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    let key = sqlx::query_as::<_, KeyRow>(
        "UPDATE image_api_keys
         SET status = 'revoked', revoked_at = COALESCE(revoked_at, now())
         WHERE id = $1 AND store_id = $2
         RETURNING id, store_id, name, client_id, secret_hash, access, status, revoked_at, created_at",
    )
    .bind(key_id)
    .bind(store.id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "IMAGE_KEY_NOT_FOUND",
        message: "Image client key not found.",
    })?;
    Ok(Json(key_response(&key)))
}

pub async fn developer_store(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<ImageStoreResponse>, AppError> {
    let key = authenticate_developer(&state, &headers).await?;
    ensure_active(&key)?;
    ensure_permission(&key, Permission::List)?;
    let store = load_store_by_id(&state, key.store_id).await?;
    Ok(Json(store_response(&state, &store).await?))
}

pub async fn upload_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ImageObjectResponse>), AppError> {
    let key = authenticate_developer(&state, &headers).await?;
    ensure_active(&key)?;
    ensure_permission(&key, Permission::Upload)?;
    let object = store_object(&state, key.store_id, &headers, &body).await?;
    Ok((StatusCode::CREATED, Json(object)))
}

pub async fn session_upload_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id)): Path<(String, String, Uuid)>,
    body: Bytes,
) -> Result<(StatusCode, Json<ImageObjectResponse>), AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    let object = store_object(&state, store.id, &headers, &body).await?;
    Ok((StatusCode::CREATED, Json(object)))
}

pub(crate) async fn list_objects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<ListParams>,
) -> Result<Json<ImageListResponse>, AppError> {
    let key = authenticate_developer(&state, &headers).await?;
    ensure_active(&key)?;
    ensure_permission(&key, Permission::List)?;
    Ok(Json(
        list_store_objects(
            &state,
            key.store_id,
            params.folder.as_deref(),
            params.recursive,
        )
        .await?,
    ))
}

pub(crate) async fn session_list_objects(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id)): Path<(String, String, Uuid)>,
    Query(params): Query<ListParams>,
) -> Result<Json<ImageListResponse>, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        false,
    )
    .await?;
    Ok(Json(
        list_store_objects(&state, store.id, params.folder.as_deref(), params.recursive).await?,
    ))
}

pub async fn delete_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(image_id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let key = authenticate_developer(&state, &headers).await?;
    ensure_active(&key)?;
    ensure_permission(&key, Permission::Delete)?;
    remove_object(&state, key.store_id, image_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn session_delete_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id, image_id)): Path<(String, String, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    remove_object(&state, store.id, image_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_folder(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<FolderParams>,
) -> Result<Json<DeletedFolderResponse>, AppError> {
    let key = authenticate_developer(&state, &headers).await?;
    ensure_active(&key)?;
    ensure_permission(&key, Permission::Delete)?;
    Ok(Json(
        remove_folder(&state, key.store_id, &params.folder).await?,
    ))
}

pub(crate) async fn session_delete_folder(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id)): Path<(String, String, Uuid)>,
    Query(params): Query<FolderParams>,
) -> Result<Json<DeletedFolderResponse>, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    Ok(Json(remove_folder(&state, store.id, &params.folder).await?))
}

pub(crate) async fn sign_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(image_id): Path<Uuid>,
    Json(body): Json<SignBody>,
) -> Result<Json<SignedImageResponse>, AppError> {
    let key = authenticate_developer(&state, &headers).await?;
    ensure_active(&key)?;
    let store = load_store_by_id(&state, key.store_id).await?;
    ensure_object_in_store(&state, store.id, image_id).await?;
    let permission = match body.visibility.as_str() {
        "public" => Permission::SignPublic,
        "private" => Permission::SignPrivate,
        _ => {
            return Err(validation(
                "visibility",
                "Visibility must be public or private.",
            ));
        }
    };
    ensure_permission(&key, permission)?;
    Ok(Json(
        issue_signature(&state, &store, image_id, &body, Some(&key)).await?,
    ))
}

pub(crate) async fn session_sign_object(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_id, project_slug, store_id, image_id)): Path<(String, String, Uuid, Uuid)>,
    Json(body): Json<SignBody>,
) -> Result<Json<SignedImageResponse>, AppError> {
    let store = session_store(
        &state,
        &headers,
        &workspace_id,
        &project_slug,
        store_id,
        true,
    )
    .await?;
    ensure_object_in_store(&state, store.id, image_id).await?;
    let key = match body.visibility.as_str() {
        "private" => {
            let key_id = body.key_id.ok_or_else(|| {
                validation(
                    "keyId",
                    "Choose an active full-access key to sign a private URL.",
                )
            })?;
            let key = load_key(&state, store.id, key_id).await?;
            ensure_active(&key)?;
            ensure_permission(&key, Permission::SignPrivate)?;
            Some(key)
        }
        "public" => None,
        _ => {
            return Err(validation(
                "visibility",
                "Visibility must be public or private.",
            ));
        }
    };
    Ok(Json(
        issue_signature(&state, &store, image_id, &body, key.as_ref()).await?,
    ))
}

pub async fn serve(
    State(state): State<AppState>,
    Path((store_id, image_id)): Path<(Uuid, Uuid)>,
    request: Request<Body>,
) -> Result<Response, AppError> {
    let query = request.uri().query().unwrap_or("");
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    serve_image_query(&state, store_id, image_id, query, if_none_match.as_deref()).await
}

pub fn is_public_image_path(path: &str) -> bool {
    path.starts_with("/api/v1/images/") || path.starts_with("/images/v1/")
}

pub async fn public_cors(request: Request<Body>, next: Next) -> Response {
    let is_image = is_public_image_path(request.uri().path());
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("*"));
    if is_image && request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        apply_image_cors(response.headers_mut(), &origin);
        return response;
    }
    let mut response = next.run(request).await;
    if is_image {
        apply_image_cors(response.headers_mut(), &origin);
    }
    response
}

fn apply_image_cors(headers: &mut HeaderMap, origin: &HeaderValue) {
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static(
            "authorization, content-type, x-knotree-client-id, x-knotree-client-secret, x-knotree-folder, x-knotree-file-name",
        ),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("86400"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
}

async fn session_project(
    state: &AppState,
    headers: &HeaderMap,
    workspace_id: &str,
    project_slug: &str,
    mutate: bool,
) -> Result<Uuid, AppError> {
    if mutate {
        security::require_csrf(headers, &state.config)?;
    }
    let user = auth::authenticate(state, headers).await?;
    projects::accessible_project_id(state, user.id, workspace_id, project_slug).await
}

async fn session_store(
    state: &AppState,
    headers: &HeaderMap,
    workspace_id: &str,
    project_slug: &str,
    store_id: Uuid,
    mutate: bool,
) -> Result<StoreRow, AppError> {
    let project_id = session_project(state, headers, workspace_id, project_slug, mutate).await?;
    sqlx::query_as::<_, StoreRow>(
        "SELECT id, project_id, name, compression_mode, max_width, max_height, quality
         FROM image_stores WHERE id = $1 AND project_id = $2",
    )
    .bind(store_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "IMAGE_STORE_NOT_FOUND",
        message: "Image store not found.",
    })
}

async fn load_store_by_id(state: &AppState, store_id: Uuid) -> Result<StoreRow, AppError> {
    sqlx::query_as::<_, StoreRow>(
        "SELECT id, project_id, name, compression_mode, max_width, max_height, quality
         FROM image_stores WHERE id = $1",
    )
    .bind(store_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "IMAGE_STORE_NOT_FOUND",
        message: "Image store not found.",
    })
}

async fn load_key(state: &AppState, store_id: Uuid, key_id: Uuid) -> Result<KeyRow, AppError> {
    sqlx::query_as::<_, KeyRow>(
        "SELECT id, store_id, name, client_id, secret_hash, access, status, revoked_at, created_at
         FROM image_api_keys WHERE id = $1 AND store_id = $2",
    )
    .bind(key_id)
    .bind(store_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "IMAGE_KEY_NOT_FOUND",
        message: "Image client key not found.",
    })
}

async fn store_response(
    state: &AppState,
    store: &StoreRow,
) -> Result<ImageStoreResponse, AppError> {
    let object_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM image_objects WHERE store_id = $1",
    )
    .bind(store.id)
    .fetch_one(&state.db)
    .await?;
    let byte_size: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(byte_size), 0)::bigint FROM image_objects WHERE store_id = $1",
    )
    .bind(store.id)
    .fetch_one(&state.db)
    .await?;
    Ok(ImageStoreResponse {
        id: store.id,
        name: store.name.clone(),
        resource_type: "images",
        compression_mode: store.compression_mode.clone(),
        max_width: store.max_width,
        max_height: store.max_height,
        quality: store.quality,
        object_count,
        byte_size,
        public_base_url: state.config.image_public_base_url.clone(),
    })
}

fn key_response(key: &KeyRow) -> ImageKeyResponse {
    ImageKeyResponse {
        id: key.id,
        name: key.name.clone(),
        client_id: key.client_id.clone(),
        client_secret: None,
        access: key.access.clone(),
        status: key.status.clone(),
        created_at: format_time(key.created_at),
        revoked_at: key.revoked_at.map(format_time),
    }
}

fn object_response(object: &ObjectMeta) -> ImageObjectResponse {
    ImageObjectResponse {
        id: object.id,
        folder: object.folder.clone(),
        file_name: object.file_name.clone(),
        content_type: object.content_type.clone(),
        byte_size: object.byte_size,
        width: object.width,
        height: object.height,
        created_at: format_time(object.created_at),
    }
}

async fn authenticate_developer(state: &AppState, headers: &HeaderMap) -> Result<KeyRow, AppError> {
    let (client_id, secret) = client_credentials(headers)?;
    let row = sqlx::query_as::<_, KeyRow>(
        "SELECT id, store_id, name, client_id, secret_hash, access, status, revoked_at, created_at
         FROM image_api_keys WHERE client_id = $1",
    )
    .bind(&client_id)
    .fetch_optional(&state.db)
    .await?;
    let dummy = [0_u8; 32];
    let hash = row
        .as_ref()
        .map(|key| key.secret_hash.as_slice())
        .unwrap_or(&dummy);
    let matches = constant_eq(hash, &security::token_hash(&secret));
    let Some(key) = row else {
        return Err(invalid_key());
    };
    if !matches {
        return Err(invalid_key());
    }
    Ok(key)
}

fn client_credentials(headers: &HeaderMap) -> Result<(String, String), AppError> {
    if let Some(value) = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        let Some(token) = value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
        else {
            return Err(invalid_key());
        };
        let Some((client_id, secret)) = token.split_once(':') else {
            return Err(invalid_key());
        };
        if client_id.is_empty() || secret.is_empty() {
            return Err(invalid_key());
        }
        return Ok((client_id.to_owned(), secret.to_owned()));
    }
    let client_id = header_value(headers, "x-knotree-client-id").unwrap_or_default();
    let secret = header_value(headers, "x-knotree-client-secret").unwrap_or_default();
    if client_id.is_empty() || secret.is_empty() {
        return Err(invalid_key());
    }
    Ok((client_id, secret))
}

fn ensure_active(key: &KeyRow) -> Result<(), AppError> {
    if key.status == "active" {
        Ok(())
    } else {
        Err(revoked_key())
    }
}

fn ensure_permission(key: &KeyRow, permission: Permission) -> Result<(), AppError> {
    let allowed = match key.access.as_str() {
        "full" => true,
        "browser" => matches!(
            permission,
            Permission::Upload | Permission::List | Permission::SignPublic
        ),
        _ => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(AppError::Forbidden {
            code: "IMAGE_KEY_FORBIDDEN",
            message: "This client key cannot perform that image operation.",
        })
    }
}

async fn store_object(
    state: &AppState,
    store_id: Uuid,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<ImageObjectResponse, AppError> {
    if body.is_empty() || body.len() > MAX_IMAGE_BYTES {
        return Err(AppError::BadRequest {
            code: "IMAGE_TOO_LARGE",
            message: "Upload a PNG, JPEG, GIF, or WebP image up to 10 MB.",
        });
    }
    let sniffed = sniff_image(body).ok_or(AppError::BadRequest {
        code: "IMAGE_INVALID",
        message: "Upload a PNG, JPEG, GIF, or WebP image.",
    })?;
    let declared = header_value(headers, header::CONTENT_TYPE.as_str())
        .unwrap_or_else(|| "application/octet-stream".to_owned());
    let declared = declared
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if declared != sniffed && declared != "application/octet-stream" {
        return Err(AppError::BadRequest {
            code: "IMAGE_INVALID",
            message: "The content type does not match the image bytes.",
        });
    }
    let folder = normalize_folder(&header_value(headers, "x-knotree-folder").unwrap_or_default())?;
    let file_name =
        normalize_file_name(&header_value(headers, "x-knotree-file-name").unwrap_or_default())?;
    let decoded = image::load_from_memory(body).map_err(|_| AppError::BadRequest {
        code: "IMAGE_INVALID",
        message: "The image could not be read.",
    })?;
    let width = decoded.width();
    let height = decoded.height();
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(AppError::BadRequest {
            code: "IMAGE_TOO_LARGE",
            message: "Images must be at most 8192 pixels on a side and 16 million pixels.",
        });
    }

    let mut tx = state.db.begin().await?;
    let locked: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM image_stores WHERE id = $1 FOR UPDATE")
            .bind(store_id)
            .fetch_optional(&mut *tx)
            .await?;
    if locked.is_none() {
        return Err(AppError::NotFound {
            code: "IMAGE_STORE_NOT_FOUND",
            message: "Image store not found.",
        });
    }
    let object_count: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM image_objects WHERE store_id = $1",
    )
    .bind(store_id)
    .fetch_one(&mut *tx)
    .await?;
    let used: i64 = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(byte_size), 0)::bigint FROM image_objects WHERE store_id = $1",
    )
    .bind(store_id)
    .fetch_one(&mut *tx)
    .await?;
    if object_count >= MAX_OBJECTS_PER_STORE
        || used.saturating_add(body.len() as i64) > MAX_STORE_BYTES
    {
        return Err(AppError::Conflict {
            code: "IMAGE_QUOTA",
            message: "This image store has reached its storage quota.",
        });
    }
    let id = Uuid::new_v4();
    let inserted = sqlx::query_as::<_, ObjectMeta>(
        "INSERT INTO image_objects
            (id, store_id, folder, file_name, content_type, byte_size, width, height, data)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
         RETURNING id, folder, file_name, content_type, byte_size, width, height, created_at",
    )
    .bind(id)
    .bind(store_id)
    .bind(&folder)
    .bind(&file_name)
    .bind(sniffed)
    .bind(body.len() as i64)
    .bind(width as i32)
    .bind(height as i32)
    .bind(body)
    .fetch_one(&mut *tx)
    .await
    .map_err(map_insert)?;
    tx.commit().await?;
    Ok(object_response(&inserted))
}

async fn list_store_objects(
    state: &AppState,
    store_id: Uuid,
    folder: Option<&str>,
    recursive: bool,
) -> Result<ImageListResponse, AppError> {
    let folder = folder.map(normalize_folder).transpose()?;
    let objects =
        match folder.as_deref() {
            None => sqlx::query_as::<_, ObjectMeta>(
                "SELECT id, folder, file_name, content_type, byte_size, width, height, created_at
                 FROM image_objects WHERE store_id = $1
                 ORDER BY folder ASC, file_name ASC, id ASC",
            )
            .bind(store_id)
            .fetch_all(&state.db)
            .await?,
            Some(folder) if recursive && folder.is_empty() => sqlx::query_as::<_, ObjectMeta>(
                "SELECT id, folder, file_name, content_type, byte_size, width, height, created_at
                 FROM image_objects WHERE store_id = $1
                 ORDER BY folder ASC, file_name ASC, id ASC",
            )
            .bind(store_id)
            .fetch_all(&state.db)
            .await?,
            Some(folder) if recursive => sqlx::query_as::<_, ObjectMeta>(
                "SELECT id, folder, file_name, content_type, byte_size, width, height, created_at
                 FROM image_objects
                 WHERE store_id = $1 AND (folder = $2 OR folder LIKE $3)
                 ORDER BY folder ASC, file_name ASC, id ASC",
            )
            .bind(store_id)
            .bind(folder)
            .bind(format!("{folder}/%"))
            .fetch_all(&state.db)
            .await?,
            Some(folder) => sqlx::query_as::<_, ObjectMeta>(
                "SELECT id, folder, file_name, content_type, byte_size, width, height, created_at
                 FROM image_objects WHERE store_id = $1 AND folder = $2
                 ORDER BY file_name ASC, id ASC",
            )
            .bind(store_id)
            .bind(folder)
            .fetch_all(&state.db)
            .await?,
        };
    let all_folders = sqlx::query_scalar::<_, String>(
        "SELECT DISTINCT folder FROM image_objects WHERE store_id = $1 AND folder <> ''",
    )
    .bind(store_id)
    .fetch_all(&state.db)
    .await?;
    Ok(ImageListResponse {
        objects: objects.iter().map(object_response).collect(),
        folders: folder_tree(&all_folders),
    })
}

async fn remove_object(state: &AppState, store_id: Uuid, image_id: Uuid) -> Result<(), AppError> {
    let deleted = sqlx::query("DELETE FROM image_objects WHERE id = $1 AND store_id = $2")
        .bind(image_id)
        .bind(store_id)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() == 0 {
        return Err(AppError::NotFound {
            code: "IMAGE_NOT_FOUND",
            message: "Image not found.",
        });
    }
    Ok(())
}

async fn remove_folder(
    state: &AppState,
    store_id: Uuid,
    folder: &str,
) -> Result<DeletedFolderResponse, AppError> {
    let folder = normalize_folder(folder)?;
    if folder.is_empty() {
        return Err(validation(
            "folder",
            "Choose a folder. Delete root images individually.",
        ));
    }
    let deleted = sqlx::query(
        "DELETE FROM image_objects WHERE store_id = $1 AND (folder = $2 OR folder LIKE $3)",
    )
    .bind(store_id)
    .bind(&folder)
    .bind(format!("{folder}/%"))
    .execute(&state.db)
    .await?;
    Ok(DeletedFolderResponse {
        deleted: deleted.rows_affected(),
    })
}

async fn ensure_object_in_store(
    state: &AppState,
    store_id: Uuid,
    image_id: Uuid,
) -> Result<(), AppError> {
    let exists: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM image_objects WHERE id = $1 AND store_id = $2")
            .bind(image_id)
            .bind(store_id)
            .fetch_optional(&state.db)
            .await?;
    exists.map(|_| ()).ok_or(AppError::NotFound {
        code: "IMAGE_NOT_FOUND",
        message: "Image not found.",
    })
}

async fn issue_signature(
    state: &AppState,
    store: &StoreRow,
    image_id: Uuid,
    body: &SignBody,
    key: Option<&KeyRow>,
) -> Result<SignedImageResponse, AppError> {
    let visibility = match body.visibility.as_str() {
        "public" | "private" => body.visibility.clone(),
        _ => {
            return Err(validation(
                "visibility",
                "Visibility must be public or private.",
            ));
        }
    };
    let (exp, kid) = if visibility == "private" {
        let seconds = body.expires_in_seconds.ok_or_else(|| {
            validation(
                "expiresInSeconds",
                "Private URLs need an expiry from 1 second to 7 days.",
            )
        })?;
        if !(1..=PRIVATE_MAX_EXPIRY_SECONDS).contains(&seconds) {
            return Err(validation(
                "expiresInSeconds",
                "Private URLs need an expiry from 1 second to 7 days.",
            ));
        }
        let key = key.ok_or_else(|| {
            validation(
                "keyId",
                "Choose an active full-access key to sign a private URL.",
            )
        })?;
        let exp = OffsetDateTime::now_utc().unix_timestamp() + seconds;
        (Some(exp), Some(key.client_id.clone()))
    } else {
        if body.expires_in_seconds.is_some() {
            return Err(validation(
                "expiresInSeconds",
                "Public URLs are stable and do not expire.",
            ));
        }
        (None, None)
    };
    let (mode, width, height, quality) = signed_transform(store, body)?;
    let variant = SignedVariant {
        visibility: visibility.clone(),
        mode,
        width,
        height,
        quality,
        exp,
        kid,
    };
    let payload = canonical(store.id, image_id, &variant);
    let signature = sign_payload(&state.config.image_url_signing_key, &payload);
    let url = image_url(
        &state.config.image_public_base_url,
        store.id,
        image_id,
        &variant,
        &signature,
    );
    let content_type = if variant.mode == "none" {
        object_content_type(state, store.id, image_id).await?
    } else {
        "image/webp".to_owned()
    };
    let expires_at = variant
        .exp
        .and_then(|exp| OffsetDateTime::from_unix_timestamp(exp).ok())
        .map(format_time);
    Ok(SignedImageResponse {
        url,
        visibility,
        expires_at,
        cache_seconds: if variant.visibility == "public" {
            PUBLIC_CACHE_SECONDS
        } else {
            PRIVATE_CACHE_SECONDS
        },
        content_type,
    })
}

fn signed_transform(
    store: &StoreRow,
    body: &SignBody,
) -> Result<(String, Option<u32>, Option<u32>, Option<u32>), AppError> {
    match store.compression_mode.as_str() {
        "none" => {
            if body.width.is_some() || body.height.is_some() || body.quality.is_some() {
                return Err(validation(
                    "compressionMode",
                    "This store keeps the original format. Remove width, height, and quality.",
                ));
            }
            Ok(("none".to_owned(), None, None, None))
        }
        "fixed" => {
            if body.width.is_some() || body.height.is_some() || body.quality.is_some() {
                return Err(validation(
                    "compressionMode",
                    "This store compresses every image with its saved WebP settings.",
                ));
            }
            Ok((
                "fixed".to_owned(),
                store.max_width.map(|value| value as u32),
                store.max_height.map(|value| value as u32),
                Some(store.quality.unwrap_or(80) as u32),
            ))
        }
        "per_url" => {
            let quality = body.quality.unwrap_or(store.quality.unwrap_or(80) as u32);
            if !(1..=100).contains(&quality) {
                return Err(validation("quality", "Quality must be from 1 to 100."));
            }
            if let Some(width) = body.width {
                validate_requested_dimension("width", width, store.max_width)?;
            }
            if let Some(height) = body.height {
                validate_requested_dimension("height", height, store.max_height)?;
            }
            Ok(("per_url".to_owned(), body.width, body.height, Some(quality)))
        }
        _ => Err(AppError::internal(
            "image store compression mode is invalid",
        )),
    }
}

fn validate_requested_dimension(field: &str, value: u32, cap: Option<i32>) -> Result<(), AppError> {
    if !(1..=MAX_DIMENSION).contains(&value) {
        return Err(validation(field, "Size must be from 1 to 8192 pixels."));
    }
    if cap.is_some_and(|cap| value > cap as u32) {
        return Err(validation(
            field,
            "That size is larger than this store allows.",
        ));
    }
    Ok(())
}

async fn object_content_type(
    state: &AppState,
    store_id: Uuid,
    image_id: Uuid,
) -> Result<String, AppError> {
    sqlx::query_scalar("SELECT content_type FROM image_objects WHERE id = $1 AND store_id = $2")
        .bind(image_id)
        .bind(store_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound {
            code: "IMAGE_NOT_FOUND",
            message: "Image not found.",
        })
}

async fn serve_image_query(
    state: &AppState,
    store_id: Uuid,
    image_id: Uuid,
    query: &str,
    if_none_match: Option<&str>,
) -> Result<Response, AppError> {
    let variant = parse_signed_query(query)?;
    let provided = query_value(query, "sig").ok_or_else(invalid_url)?;
    let payload = canonical(store_id, image_id, &variant);
    let expected = sign_payload(&state.config.image_url_signing_key, &payload);
    if !signatures_match(&expected, &provided) {
        return Err(invalid_url());
    }
    if variant.visibility == "private" {
        let exp = variant.exp.ok_or_else(invalid_url)?;
        if OffsetDateTime::now_utc().unix_timestamp() >= exp {
            return Err(AppError::Unauthorized {
                code: "IMAGE_URL_EXPIRED",
                message: "This private image URL has expired.",
            });
        }
        let kid = variant.kid.as_deref().ok_or_else(invalid_url)?;
        let status: Option<String> = sqlx::query_scalar(
            "SELECT status FROM image_api_keys WHERE client_id = $1 AND store_id = $2",
        )
        .bind(kid)
        .bind(store_id)
        .fetch_optional(&state.db)
        .await?;
        if status.as_deref() != Some("active") {
            return Err(revoked_key());
        }
    }
    let object = sqlx::query_as::<_, ObjectBlob>(
        "SELECT content_type, data FROM image_objects WHERE id = $1 AND store_id = $2",
    )
    .bind(image_id)
    .bind(store_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "IMAGE_NOT_FOUND",
        message: "Image not found.",
    })?;
    let (bytes, content_type) = render_image(
        &object.data,
        &object.content_type,
        &variant.mode,
        variant.width,
        variant.height,
        variant.quality,
    )?;
    image_http_response(bytes, &content_type, &variant.visibility, if_none_match)
}

fn image_http_response(
    bytes: Vec<u8>,
    content_type: &str,
    visibility: &str,
    if_none_match: Option<&str>,
) -> Result<Response, AppError> {
    let etag = format!("\"{}\"", hex::encode(Sha256::digest(&bytes)));
    let cache_control = cache_control_value(visibility);
    let cdn_cache_control = cdn_cache_control_value(visibility);
    if if_none_match.is_some_and(|value| value == etag) {
        return Ok(Response::builder()
            .status(StatusCode::NOT_MODIFIED)
            .header(header::ETAG, etag)
            .header(header::CACHE_CONTROL, cache_control)
            .header("cdn-cache-control", cdn_cache_control)
            .body(Body::empty())
            .map_err(AppError::internal)?);
    }
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, "inline")
        .header("x-content-type-options", "nosniff")
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, cache_control)
        .header("cdn-cache-control", cdn_cache_control)
        .header("x-knotree-image-visibility", visibility)
        .body(Body::from(bytes))
        .map_err(AppError::internal)?)
}

fn cache_control_value(visibility: &str) -> &'static str {
    if visibility == "public" {
        "public, max-age=31536000, immutable"
    } else {
        "public, max-age=60"
    }
}

fn cdn_cache_control_value(visibility: &str) -> &'static str {
    if visibility == "public" {
        "public, max-age=31536000"
    } else {
        "public, max-age=60"
    }
}

fn parse_signed_query(query: &str) -> Result<SignedVariant, AppError> {
    let mut values = BTreeMap::new();
    if !query.is_empty() {
        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                return Err(invalid_url());
            };
            if value.is_empty() || values.insert(key.to_owned(), value.to_owned()).is_some() {
                return Err(invalid_url());
            }
        }
    }
    const ALLOWED: &[&str] = &["mode", "w", "h", "q", "exp", "kid", "sig"];
    if values.keys().any(|key| !ALLOWED.contains(&key.as_str())) || !values.contains_key("sig") {
        return Err(invalid_url());
    }
    let mode = values.get("mode").ok_or_else(invalid_url)?.to_owned();
    if !matches!(mode.as_str(), "none" | "fixed" | "per_url") {
        return Err(invalid_url());
    }
    let width = optional_query_u32(&values, "w")?;
    let height = optional_query_u32(&values, "h")?;
    let quality = optional_query_u32(&values, "q")?;
    let exp = optional_query_i64(&values, "exp")?;
    let kid = values.get("kid").cloned();
    if mode == "none" && (width.is_some() || height.is_some() || quality.is_some()) {
        return Err(invalid_url());
    }
    if matches!(mode.as_str(), "fixed" | "per_url") && quality.is_none() {
        return Err(invalid_url());
    }
    if quality.is_some_and(|quality| !(1..=100).contains(&quality)) {
        return Err(invalid_url());
    }
    let visibility = if exp.is_some() || kid.is_some() {
        if exp.is_none() || kid.is_none() {
            return Err(invalid_url());
        }
        "private".to_owned()
    } else {
        "public".to_owned()
    };
    Ok(SignedVariant {
        visibility,
        mode,
        width,
        height,
        quality,
        exp,
        kid,
    })
}

fn optional_query_u32(
    values: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<u32>, AppError> {
    match values.get(key) {
        None => Ok(None),
        Some(value) => {
            let parsed = value.parse::<u32>().map_err(|_| invalid_url())?;
            if parsed == 0 || parsed > MAX_DIMENSION {
                return Err(invalid_url());
            }
            Ok(Some(parsed))
        }
    }
}

fn optional_query_i64(
    values: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<i64>, AppError> {
    match values.get(key) {
        None => Ok(None),
        Some(value) => Ok(Some(value.parse::<i64>().map_err(|_| invalid_url())?)),
    }
}

fn query_value(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (name, value) = pair.split_once('=')?;
        (name == key).then(|| value.to_owned())
    })
}

fn canonical(store_id: Uuid, image_id: Uuid, variant: &SignedVariant) -> String {
    format!(
        "v1\n{}\n{store_id}\n{image_id}\n{}\n{}\n{}\n{}\n{}\n{}",
        variant.visibility,
        variant.mode,
        display_optional(variant.width),
        display_optional(variant.height),
        display_optional(variant.quality),
        display_optional(variant.exp),
        variant.kid.as_deref().unwrap_or(""),
    )
}

fn display_optional(value: Option<impl ToString>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn sign_payload(key: &[u8; 32], payload: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key).expect("image signing key is 32 bytes");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn signatures_match(expected_hex: &str, provided_hex: &str) -> bool {
    let Ok(expected) = hex::decode(expected_hex) else {
        return false;
    };
    let Ok(provided) = hex::decode(provided_hex) else {
        return false;
    };
    constant_eq(&expected, &provided)
}

fn image_url(
    base: &str,
    store_id: Uuid,
    image_id: Uuid,
    variant: &SignedVariant,
    signature: &str,
) -> String {
    let mut pairs = vec![format!("mode={}", variant.mode)];
    if let Some(width) = variant.width {
        pairs.push(format!("w={width}"));
    }
    if let Some(height) = variant.height {
        pairs.push(format!("h={height}"));
    }
    if let Some(quality) = variant.quality {
        pairs.push(format!("q={quality}"));
    }
    if let Some(exp) = variant.exp {
        pairs.push(format!("exp={exp}"));
    }
    if let Some(kid) = &variant.kid {
        pairs.push(format!("kid={kid}"));
    }
    pairs.push(format!("sig={signature}"));
    format!(
        "{}/images/v1/{store_id}/{image_id}?{}",
        base.trim_end_matches('/'),
        pairs.join("&")
    )
}

fn render_image(
    bytes: &[u8],
    content_type: &str,
    mode: &str,
    width: Option<u32>,
    height: Option<u32>,
    quality: Option<u32>,
) -> Result<(Vec<u8>, String), AppError> {
    if mode == "none" {
        return Ok((bytes.to_vec(), content_type.to_owned()));
    }
    let image = image::load_from_memory(bytes).map_err(|_| AppError::BadRequest {
        code: "IMAGE_TRANSFORM_FAILED",
        message: "The stored image could not be converted to WebP.",
    })?;
    let (target_width, target_height) =
        target_dimensions(image.width(), image.height(), width, height);
    let resized = if target_width == image.width() && target_height == image.height() {
        image
    } else {
        image.resize_exact(
            target_width,
            target_height,
            image::imageops::FilterType::Lanczos3,
        )
    };
    let rgba = resized.to_rgba8();
    let encoded = encode_webp(&rgba, quality.unwrap_or(80))?;
    Ok((encoded, "image/webp".to_owned()))
}

fn target_dimensions(
    width: u32,
    height: u32,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> (u32, u32) {
    let mut scale = 1.0_f64;
    if let Some(max_width) = max_width.filter(|value| *value > 0 && *value < width) {
        scale = scale.min(f64::from(max_width) / f64::from(width));
    }
    if let Some(max_height) = max_height.filter(|value| *value > 0 && *value < height) {
        scale = scale.min(f64::from(max_height) / f64::from(height));
    }
    if scale >= 1.0 {
        return (width.max(1), height.max(1));
    }
    let target_width = (f64::from(width) * scale).round().max(1.0) as u32;
    let target_height = (f64::from(height) * scale).round().max(1.0) as u32;
    (target_width, target_height)
}

fn encode_webp(image: &image::RgbaImage, quality: u32) -> Result<Vec<u8>, AppError> {
    let quality = quality.clamp(1, 100) as f32;
    let encoder = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height());
    Ok(encoder.encode(quality).to_vec())
}

fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    None
}

fn normalize_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        return Err(validation("name", "Enter a name up to 80 characters."));
    }
    Ok(name.to_owned())
}

fn normalize_access(access: &str) -> Result<String, AppError> {
    match access {
        "full" | "browser" => Ok(access.to_owned()),
        _ => Err(validation("access", "Access must be full or browser.")),
    }
}

fn normalize_folder(folder: &str) -> Result<String, AppError> {
    let folder = folder.trim().trim_matches('/');
    if folder.is_empty() {
        return Ok(String::new());
    }
    if folder.len() > 512 {
        return Err(validation(
            "folder",
            "Folder paths must be 512 characters or fewer.",
        ));
    }
    let mut parts = Vec::new();
    for part in folder.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.len() > 80
            || !part.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
            })
        {
            return Err(validation(
                "folder",
                "Folders use letters, numbers, dots, underscores, and hyphens.",
            ));
        }
        parts.push(part);
    }
    if parts.len() > 16 {
        return Err(validation(
            "folder",
            "Folders can be at most 16 levels deep.",
        ));
    }
    Ok(parts.join("/"))
}

fn normalize_file_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty()
        || name.len() > 180
        || name.contains('/')
        || name.contains('\\')
        || name == "."
        || name == ".."
        || name.chars().any(char::is_control)
    {
        return Err(validation(
            "fileName",
            "Enter a file name up to 180 characters without slashes.",
        ));
    }
    Ok(name.to_owned())
}

fn normalize_compression(
    mode: &str,
    max_width: Option<u32>,
    max_height: Option<u32>,
    quality: Option<u32>,
) -> Result<CompressionSettings, AppError> {
    match mode {
        "none" => {
            if max_width.is_some() || max_height.is_some() || quality.is_some() {
                return Err(validation(
                    "compressionMode",
                    "Original images do not take width, height, or quality.",
                ));
            }
            Ok(CompressionSettings {
                mode: "none".to_owned(),
                max_width: None,
                max_height: None,
                quality: None,
            })
        }
        "fixed" | "per_url" => {
            let quality = quality.unwrap_or(80);
            if !(1..=100).contains(&quality) {
                return Err(validation("quality", "Quality must be from 1 to 100."));
            }
            Ok(CompressionSettings {
                mode: mode.to_owned(),
                max_width: optional_dimension("maxWidth", max_width)?,
                max_height: optional_dimension("maxHeight", max_height)?,
                quality: Some(quality as i32),
            })
        }
        _ => Err(validation(
            "compressionMode",
            "Choose none, fixed, or per_url.",
        )),
    }
}

fn optional_dimension(field: &str, value: Option<u32>) -> Result<Option<i32>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if (1..=MAX_DIMENSION).contains(&value) {
        Ok(Some(value as i32))
    } else {
        Err(validation(field, "Size must be from 1 to 8192 pixels."))
    }
}

fn folder_tree(folders: &[String]) -> Vec<String> {
    let mut tree = BTreeSet::new();
    for folder in folders {
        let mut prefix = String::new();
        for part in folder.split('/') {
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            tree.insert(prefix.clone());
        }
    }
    tree.into_iter().collect()
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn format_time(value: OffsetDateTime) -> String {
    value
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

fn validation(field: &str, message: &str) -> AppError {
    let mut fields = BTreeMap::new();
    fields.insert(field.to_owned(), message.to_owned());
    AppError::validation(fields)
}

fn invalid_key() -> AppError {
    AppError::Unauthorized {
        code: "IMAGE_KEY_INVALID",
        message: "The client id or client secret is invalid.",
    }
}

fn revoked_key() -> AppError {
    AppError::Unauthorized {
        code: "IMAGE_KEY_REVOKED",
        message: "This client key has been revoked.",
    }
}

fn invalid_url() -> AppError {
    AppError::Unauthorized {
        code: "IMAGE_URL_INVALID",
        message: "The image URL is invalid.",
    }
}

fn constant_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).into()
}

fn map_insert(error: sqlx::Error) -> AppError {
    match &error {
        sqlx::Error::Database(db) if db.code().as_deref() == Some("23505") => AppError::Conflict {
            code: "IMAGE_NAME_TAKEN",
            message: "An image with that folder and file name already exists.",
        },
        _ => AppError::from(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn sample_png(width: u32, height: u32) -> Vec<u8> {
        let mut image = image::RgbaImage::new(width, height);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = image::Rgba([(x * 13) as u8, (y * 29) as u8, 180, 255]);
        }
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("png");
        bytes
    }

    fn upload_headers(content_type: &str, folder: &str, file_name: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_str(content_type).unwrap(),
        );
        if !folder.is_empty() {
            headers.insert("x-knotree-folder", HeaderValue::from_str(folder).unwrap());
        }
        headers.insert(
            "x-knotree-file-name",
            HeaderValue::from_str(file_name).unwrap(),
        );
        headers
    }

    fn developer_headers(client_id: &str, client_secret: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-knotree-client-id",
            HeaderValue::from_str(client_id).unwrap(),
        );
        headers.insert(
            "x-knotree-client-secret",
            HeaderValue::from_str(client_secret).unwrap(),
        );
        headers
    }

    #[test]
    fn folders_reject_traversal_and_keep_nested_names() {
        assert_eq!(normalize_folder("").unwrap(), "");
        assert_eq!(
            normalize_folder("/products/covers/").unwrap(),
            "products/covers"
        );
        assert!(normalize_folder("../secrets").is_err());
        assert!(normalize_folder("products//covers").is_err());
        assert!(normalize_file_name("hero.png").is_ok());
        assert!(normalize_file_name("../hero.png").is_err());
    }

    #[test]
    fn resize_fits_inside_the_requested_box_without_upscaling() {
        assert_eq!(target_dimensions(800, 400, Some(200), None), (200, 100));
        assert_eq!(target_dimensions(80, 40, Some(400), Some(400)), (80, 40));
        assert_eq!(target_dimensions(100, 100, Some(50), Some(20)), (20, 20));
    }

    #[test]
    fn webp_encoder_respects_size_and_quality() {
        let png = sample_png(48, 24);
        let (small, content_type) =
            render_image(&png, "image/png", "per_url", Some(16), None, Some(20)).unwrap();
        let (large, _) =
            render_image(&png, "image/png", "per_url", Some(16), None, Some(95)).unwrap();
        assert_eq!(content_type, "image/webp");
        assert!(small.starts_with(b"RIFF") && small.windows(4).any(|chunk| chunk == b"WEBP"));
        assert_ne!(small, large);
        let decoded = image::load_from_memory(&small).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (16, 8));
        let (original, original_type) =
            render_image(&png, "image/png", "none", None, None, None).unwrap();
        assert_eq!(original_type, "image/png");
        assert_eq!(original, png);
    }

    #[test]
    fn public_cache_is_one_year_and_private_cache_is_short() {
        assert_eq!(
            cache_control_value("public"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(
            cdn_cache_control_value("public"),
            "public, max-age=31536000"
        );
        assert_eq!(cache_control_value("private"), "public, max-age=60");
        assert_eq!(cdn_cache_control_value("private"), "public, max-age=60");
        assert!(is_public_image_path("/api/v1/images/objects"));
        assert!(is_public_image_path("/images/v1/store/image"));
        assert!(!is_public_image_path(
            "/api/v1/workspaces/a/projects/b/image-stores"
        ));
    }

    #[tokio::test]
    async fn public_urls_keep_working_after_key_revocation() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let seed = crate::test_support::seed_owner_project(&state).await;
        let session = crate::test_support::session_headers(&state, seed.user_id).await;
        let (_, Json(store)) = create_store(
            State(state.clone()),
            session.clone(),
            Path((seed.workspace_route_id.clone(), seed.project_slug.clone())),
            Json(SaveStoreBody {
                name: "Website".to_owned(),
                compression_mode: "per_url".to_owned(),
                max_width: Some(64),
                max_height: None,
                quality: Some(80),
            }),
        )
        .await
        .unwrap();
        let (_, Json(key)) = create_key(
            State(state.clone()),
            session.clone(),
            Path((
                seed.workspace_route_id.clone(),
                seed.project_slug.clone(),
                store.id,
            )),
            Json(CreateKeyBody {
                name: "server".to_owned(),
                access: "full".to_owned(),
            }),
        )
        .await
        .unwrap();
        let secret = key.client_secret.clone().unwrap();
        let mut headers = developer_headers(&key.client_id, &secret);
        headers.extend(upload_headers("image/png", "posts/covers", "hero.png"));
        let (_, Json(uploaded)) = upload_object(
            State(state.clone()),
            headers.clone(),
            Bytes::from(sample_png(32, 16)),
        )
        .await
        .unwrap();
        assert_eq!(uploaded.folder, "posts/covers");

        let Json(first) = sign_object(
            State(state.clone()),
            developer_headers(&key.client_id, &secret),
            Path(uploaded.id),
            Json(SignBody {
                visibility: "public".to_owned(),
                expires_in_seconds: None,
                width: Some(16),
                height: None,
                quality: Some(70),
                key_id: None,
            }),
        )
        .await
        .unwrap();
        let Json(second) = sign_object(
            State(state.clone()),
            developer_headers(&key.client_id, &secret),
            Path(uploaded.id),
            Json(SignBody {
                visibility: "public".to_owned(),
                expires_in_seconds: None,
                width: Some(16),
                height: None,
                quality: Some(70),
                key_id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(first.url, second.url);
        assert!(first.url.contains("/images/v1/"));
        assert_eq!(first.cache_seconds, PUBLIC_CACHE_SECONDS);
        assert!(first.expires_at.is_none());
        let public_query = first.url.split_once('?').unwrap().1;
        let public_response = serve_image_query(&state, store.id, uploaded.id, public_query, None)
            .await
            .unwrap();
        assert_eq!(public_response.status(), StatusCode::OK);
        assert_eq!(
            public_response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(
            public_response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("image/webp")
        );

        let Json(private_url) = sign_object(
            State(state.clone()),
            developer_headers(&key.client_id, &secret),
            Path(uploaded.id),
            Json(SignBody {
                visibility: "private".to_owned(),
                expires_in_seconds: Some(600),
                width: Some(16),
                height: None,
                quality: Some(70),
                key_id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(private_url.cache_seconds, PRIVATE_CACHE_SECONDS);
        assert!(private_url.expires_at.is_some());
        let private_query = private_url.url.split_once('?').unwrap().1.to_owned();
        let private_response =
            serve_image_query(&state, store.id, uploaded.id, &private_query, None)
                .await
                .unwrap();
        assert_eq!(
            private_response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("public, max-age=60")
        );

        let Json(revoked) = revoke_key(
            State(state.clone()),
            session,
            Path((seed.workspace_route_id, seed.project_slug, store.id, key.id)),
        )
        .await
        .unwrap();
        assert_eq!(revoked.status, "revoked");

        let still_public = serve_image_query(&state, store.id, uploaded.id, public_query, None)
            .await
            .unwrap();
        assert_eq!(still_public.status(), StatusCode::OK);
        let private_after = serve_image_query(&state, store.id, uploaded.id, &private_query, None)
            .await
            .unwrap_err();
        assert!(matches!(
            private_after,
            AppError::Unauthorized {
                code: "IMAGE_KEY_REVOKED",
                ..
            }
        ));
        let upload_after = upload_object(
            State(state.clone()),
            {
                let mut headers = developer_headers(&key.client_id, &secret);
                headers.extend(upload_headers("image/png", "posts", "other.png"));
                headers
            },
            Bytes::from(sample_png(8, 8)),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            upload_after,
            AppError::Unauthorized {
                code: "IMAGE_KEY_REVOKED",
                ..
            }
        ));
    }

    #[tokio::test]
    async fn compression_modes_folders_and_browser_keys() {
        let Some(state) = crate::test_support::test_app_state().await else {
            return;
        };
        let seed = crate::test_support::seed_owner_project(&state).await;
        let session = crate::test_support::session_headers(&state, seed.user_id).await;
        let (_, Json(original_store)) = create_store(
            State(state.clone()),
            session.clone(),
            Path((seed.workspace_route_id.clone(), seed.project_slug.clone())),
            Json(SaveStoreBody {
                name: "Originals".to_owned(),
                compression_mode: "none".to_owned(),
                max_width: None,
                max_height: None,
                quality: None,
            }),
        )
        .await
        .unwrap();
        let (_, Json(fixed_store)) = create_store(
            State(state.clone()),
            session.clone(),
            Path((seed.workspace_route_id.clone(), seed.project_slug.clone())),
            Json(SaveStoreBody {
                name: "Fixed".to_owned(),
                compression_mode: "fixed".to_owned(),
                max_width: Some(10),
                max_height: None,
                quality: Some(60),
            }),
        )
        .await
        .unwrap();
        let (_, Json(browser_key)) = create_key(
            State(state.clone()),
            session.clone(),
            Path((
                seed.workspace_route_id.clone(),
                seed.project_slug.clone(),
                original_store.id,
            )),
            Json(CreateKeyBody {
                name: "browser".to_owned(),
                access: "browser".to_owned(),
            }),
        )
        .await
        .unwrap();
        let browser_secret = browser_key.client_secret.unwrap();
        let mut headers = developer_headers(&browser_key.client_id, &browser_secret);
        headers.extend(upload_headers("image/png", "albums/2026", "cover.png"));
        let png = sample_png(20, 10);
        let (_, Json(uploaded)) =
            upload_object(State(state.clone()), headers, Bytes::from(png.clone()))
                .await
                .unwrap();
        let Json(list) = list_objects(
            State(state.clone()),
            developer_headers(&browser_key.client_id, &browser_secret),
            Query(ListParams {
                folder: Some("albums".to_owned()),
                recursive: true,
            }),
        )
        .await
        .unwrap();
        assert_eq!(list.objects.len(), 1);
        assert_eq!(
            list.folders,
            vec!["albums".to_owned(), "albums/2026".to_owned()]
        );

        let Json(signed) = sign_object(
            State(state.clone()),
            developer_headers(&browser_key.client_id, &browser_secret),
            Path(uploaded.id),
            Json(SignBody {
                visibility: "public".to_owned(),
                expires_in_seconds: None,
                width: None,
                height: None,
                quality: None,
                key_id: None,
            }),
        )
        .await
        .unwrap();
        assert_eq!(signed.content_type, "image/png");
        let query = signed.url.split_once('?').unwrap().1;
        let response = serve_image_query(&state, original_store.id, uploaded.id, query, None)
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("image/png")
        );
        let private_denied = sign_object(
            State(state.clone()),
            developer_headers(&browser_key.client_id, &browser_secret),
            Path(uploaded.id),
            Json(SignBody {
                visibility: "private".to_owned(),
                expires_in_seconds: Some(60),
                width: None,
                height: None,
                quality: None,
                key_id: None,
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(private_denied, AppError::Forbidden { .. }));
        let delete_denied = delete_object(
            State(state.clone()),
            developer_headers(&browser_key.client_id, &browser_secret),
            Path(uploaded.id),
        )
        .await
        .unwrap_err();
        assert!(matches!(delete_denied, AppError::Forbidden { .. }));

        let (_, Json(fixed_key)) = create_key(
            State(state.clone()),
            session.clone(),
            Path((
                seed.workspace_route_id.clone(),
                seed.project_slug.clone(),
                fixed_store.id,
            )),
            Json(CreateKeyBody {
                name: "server".to_owned(),
                access: "full".to_owned(),
            }),
        )
        .await
        .unwrap();
        let fixed_secret = fixed_key.client_secret.unwrap();
        let mut fixed_headers = developer_headers(&fixed_key.client_id, &fixed_secret);
        fixed_headers.extend(upload_headers("image/png", "brand", "logo.png"));
        let (_, Json(fixed_image)) =
            upload_object(State(state.clone()), fixed_headers, Bytes::from(png))
                .await
                .unwrap();
        let custom_size = sign_object(
            State(state.clone()),
            developer_headers(&fixed_key.client_id, &fixed_secret),
            Path(fixed_image.id),
            Json(SignBody {
                visibility: "public".to_owned(),
                expires_in_seconds: None,
                width: Some(4),
                height: None,
                quality: None,
                key_id: None,
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(custom_size, AppError::Validation { .. }));
        let Json(fixed_signed) = sign_object(
            State(state.clone()),
            developer_headers(&fixed_key.client_id, &fixed_secret),
            Path(fixed_image.id),
            Json(SignBody {
                visibility: "public".to_owned(),
                expires_in_seconds: None,
                width: None,
                height: None,
                quality: None,
                key_id: None,
            }),
        )
        .await
        .unwrap();
        assert!(fixed_signed.url.contains("mode=fixed"));
        assert!(fixed_signed.url.contains("w=10"));
        assert!(fixed_signed.url.contains("q=60"));
        assert_eq!(fixed_signed.content_type, "image/webp");

        let expired = SignedVariant {
            visibility: "private".to_owned(),
            mode: "fixed".to_owned(),
            width: Some(10),
            height: None,
            quality: Some(60),
            exp: Some(OffsetDateTime::now_utc().unix_timestamp() - 5),
            kid: Some(fixed_key.client_id.clone()),
        };
        let payload = canonical(fixed_store.id, fixed_image.id, &expired);
        let signature = sign_payload(&state.config.image_url_signing_key, &payload);
        let expired_query = image_url(
            "http://localhost:8080",
            fixed_store.id,
            fixed_image.id,
            &expired,
            &signature,
        );
        let expired_query = expired_query.split_once('?').unwrap().1;
        let expired_response =
            serve_image_query(&state, fixed_store.id, fixed_image.id, expired_query, None)
                .await
                .unwrap_err();
        assert!(matches!(
            expired_response,
            AppError::Unauthorized {
                code: "IMAGE_URL_EXPIRED",
                ..
            }
        ));

        let Json(deleted) = delete_folder(
            State(state.clone()),
            developer_headers(&fixed_key.client_id, &fixed_secret),
            Query(FolderParams {
                folder: "brand".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(deleted.deleted, 1);
        let missing = serve_image_query(
            &state,
            fixed_store.id,
            fixed_image.id,
            fixed_signed.url.split_once('?').unwrap().1,
            None,
        )
        .await
        .unwrap_err();
        assert!(missing.is_not_found());

        let Json(removed) = session_delete_folder(
            State(state.clone()),
            session,
            Path((
                seed.workspace_route_id,
                seed.project_slug,
                original_store.id,
            )),
            Query(FolderParams {
                folder: "albums".to_owned(),
            }),
        )
        .await
        .unwrap();
        assert_eq!(removed.deleted, 1);
    }
}
