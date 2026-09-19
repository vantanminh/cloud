use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use axum::{
    Json,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;
use zip::ZipArchive;

use crate::{
    auth, error::AppError, github, projects, security, state::AppState,
};

pub const IMAGE_SOURCE_HTML: &str = "html";
pub const IMAGE_SOURCE_HTML_GITHUB: &str = "html_github";
pub const HTML_NGINX_IMAGE: &str = "nginxinc/nginx-unprivileged:1.27-alpine";
pub const HTML_NGINX_PORT: u16 = 8080;
const MAX_SITE_BYTES: usize = 25 * 1024 * 1024;
const MAX_FILES: usize = 800;
const MAX_INDEX_HTML_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct HtmlFile {
    pub path: String,
    pub content: Vec<u8>,
    pub content_type: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateHtmlPageRequest {
    pub index_html: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlAnalyticsEventRequest {
    pub event_type: Option<String>,
    pub path: Option<String>,
    pub referrer: Option<String>,
    pub language: Option<String>,
    pub timezone: Option<String>,
    pub screen_width: Option<i32>,
    pub screen_height: Option<i32>,
    pub viewport_width: Option<i32>,
    pub viewport_height: Option<i32>,
    pub session_id: Option<String>,
    pub duration_ms: Option<i32>,
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlAnalyticsSummary {
    pub pageviews: i64,
    pub sessions: i64,
    pub avg_duration_ms: f64,
    pub top_paths: Vec<HtmlCountRow>,
    pub top_referrers: Vec<HtmlCountRow>,
    pub browsers: Vec<HtmlCountRow>,
    pub event_types: Vec<HtmlCountRow>,
    pub recent: Vec<HtmlRecentEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlCountRow {
    pub name: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlRecentEvent {
    pub occurred_at: String,
    pub event_type: String,
    pub path: String,
    pub referrer: Option<String>,
    pub session_id: Option<String>,
}

pub fn is_html_source(source: &str) -> bool {
    source == IMAGE_SOURCE_HTML || source == IMAGE_SOURCE_HTML_GITHUB
}

pub fn page_subdomain(suffix: &str) -> Result<String, AppError> {
    let mut value = suffix.trim().to_ascii_lowercase();
    if let Some(stripped) = value.strip_prefix("page-") {
        value = stripped.to_owned();
    }
    if value.is_empty()
        || value.len() > 48
        || value.starts_with('-')
        || value.ends_with('-')
        || !value
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
    {
        let mut fields = BTreeMap::new();
        fields.insert(
            "pageSlug".to_owned(),
            "Use a unique suffix of lowercase letters, numbers, and hyphens after page-."
                .to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok(format!("page-{value}"))
}

pub fn site_dir(data_dir: &str, service_id: Uuid) -> PathBuf {
    PathBuf::from(data_dir).join(service_id.to_string())
}

pub fn nginx_conf() -> &'static str {
    r#"server {
    listen 8080;
    server_name _;
    root /usr/share/nginx/html;
    gzip on;
    gzip_types text/css application/javascript application/json image/svg+xml;

    location /__kt/ {
        add_header Cache-Control "no-store";
        return 404;
    }

    location ~* \.(?:css|js|mjs|map|png|jpe?g|gif|svg|webp|ico|woff2?|ttf|otf|mp4|webm)$ {
        add_header Cache-Control "public, max-age=604800, stale-while-revalidate=86400";
        add_header CDN-Cache-Control "public, max-age=604800";
        add_header CloudFlare-CDN-Cache-Control "max-age=604800";
        try_files $uri =404;
    }

    location / {
        add_header Cache-Control "public, max-age=60, must-revalidate";
        add_header CDN-Cache-Control "public, max-age=60";
        add_header CloudFlare-CDN-Cache-Control "max-age=60";
        try_files $uri $uri/ /index.html;
    }
}
"#
}

pub fn inject_analytics(html: &str, collect_origin: &str, site_id: Uuid) -> String {
    let snippet = format!(
        r#"<script defer src="{collect_origin}/api/v1/public/html-pages/{site_id}/analytics.js" data-kt-site="{site_id}"></script>"#
    );
    if html.contains("html-pages/") && html.contains("/analytics.js") {
        return html.to_owned();
    }
    if let Some(index) = html.to_ascii_lowercase().rfind("</head>") {
        let mut out = String::with_capacity(html.len() + snippet.len() + 1);
        out.push_str(&html[..index]);
        out.push_str(&snippet);
        out.push('\n');
        out.push_str(&html[index..]);
        return out;
    }
    if let Some(index) = html.to_ascii_lowercase().rfind("</body>") {
        let mut out = String::with_capacity(html.len() + snippet.len() + 1);
        out.push_str(&html[..index]);
        out.push_str(&snippet);
        out.push('\n');
        out.push_str(&html[index..]);
        return out;
    }
    format!("{html}\n{snippet}\n")
}

pub fn analytics_javascript(collect_origin: &str, site_id: Uuid) -> String {
    format!(
        r#"(function(){{
var SITE="{site_id}";
var ORIGIN="{collect_origin}";
var sid=localStorage.getItem("kt.sid");
if(!sid){{sid=Math.random().toString(36).slice(2)+Date.now().toString(36);localStorage.setItem("kt.sid",sid);}}
var start=Date.now();
function payload(type, extra){{
  return {{
    eventType:type,
    path:location.pathname+location.search,
    referrer:document.referrer||null,
    language:navigator.language||null,
    timezone:(Intl.DateTimeFormat().resolvedOptions().timeZone)||null,
    screenWidth:screen.width,screenHeight:screen.height,
    viewportWidth:window.innerWidth,viewportHeight:window.innerHeight,
    sessionId:sid,
    durationMs:Date.now()-start,
    extra:extra||{{}}
  }};
}}
function send(type, extra){{
  try{{
    var body=JSON.stringify(payload(type, extra));
    if(navigator.sendBeacon){{
      navigator.sendBeacon(ORIGIN+"/api/v1/public/html-pages/"+SITE+"/events", new Blob([body],{{type:"application/json"}}));
    }} else {{
      fetch(ORIGIN+"/api/v1/public/html-pages/"+SITE+"/events",{{method:"POST",headers:{{"content-type":"application/json"}},body:body,keepalive:true,mode:"cors"}});
    }}
  }}catch(e){{}}
}}
send("pageview", {{title:document.title, href:location.href}});
window.addEventListener("error", function(ev){{
  send("error", {{message:String(ev.message||"error"), source:String(ev.filename||""), line:ev.lineno||0}});
}});
document.addEventListener("click", function(ev){{
  var t=ev.target; if(!t) return;
  var el=t.closest ? t.closest("a,button") : null;
  if(!el) return;
  send("click", {{tag:el.tagName, text:(el.innerText||"").slice(0,80), href:el.getAttribute && el.getAttribute("href")}});
}}, true);
document.addEventListener("visibilitychange", function(){{ if(document.visibilityState==="hidden") send("heartbeat"); }});
window.addEventListener("pagehide", function(){{ send("session_end"); }});
}})();"#
    )
}

pub fn parse_github_repo(input: &str) -> Result<(String, String), AppError> {
    let trimmed = input.trim().trim_end_matches(".git");
    let path = if let Some(rest) = trimmed
        .strip_prefix("https://github.com/")
        .or_else(|| trimmed.strip_prefix("http://github.com/"))
        .or_else(|| trimmed.strip_prefix("git@github.com:"))
    {
        rest
    } else {
        trimmed.trim_start_matches('/')
    };
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let owner = parts.next().unwrap_or("");
    let repo = parts.next().unwrap_or("");
    if owner.is_empty()
        || repo.is_empty()
        || parts.next().is_some()
        || !owner
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        || !repo
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        let mut fields = BTreeMap::new();
        fields.insert(
            "githubRepo".to_owned(),
            "Use an owner/repo GitHub path, for example acme/docs-site.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    Ok((owner.to_owned(), repo.to_owned()))
}

pub fn detect_site_root(paths: &[String]) -> String {
    let has_root_index = paths.iter().any(|path| path == "index.html");
    if has_root_index {
        return String::new();
    }
    if paths.iter().any(|path| path == "docs/index.html") {
        return "docs".to_owned();
    }
    let mut html_dirs = paths
        .iter()
        .filter_map(|path| {
            let normalized = path.replace('\\', "/");
            if normalized.ends_with("/index.html") {
                Some(normalized.trim_end_matches("/index.html").to_owned())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    html_dirs.sort();
    html_dirs.into_iter().next().unwrap_or_default()
}

fn sanitize_relative_path(path: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    if path.is_empty() || path.ends_with('/') {
        return None;
    }
    let mut clean = PathBuf::new();
    for component in Path::new(&path).components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    let value = clean.to_string_lossy().replace('\\', "/");
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn content_type_for(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
    .to_owned()
}

fn should_skip(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            ".git" | ".github" | "node_modules" | ".DS_Store" | "Thumbs.db"
        )
    })
}

pub fn files_from_pasted_html(
    html: &str,
    collect_origin: &str,
    site_id: Uuid,
) -> Result<Vec<HtmlFile>, AppError> {
    if html.trim().is_empty() || html.len() > MAX_INDEX_HTML_BYTES {
        let mut fields = BTreeMap::new();
        fields.insert(
            "indexHtml".to_owned(),
            "Paste an index.html file up to 2 MiB.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }
    let injected = inject_analytics(html, collect_origin, site_id);
    Ok(vec![HtmlFile {
        path: "index.html".to_owned(),
        content: injected.into_bytes(),
        content_type: "text/html; charset=utf-8".to_owned(),
    }])
}

pub fn unzip_github_pages(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("invalid GitHub zipball")?;
    let mut files = Vec::new();
    let mut total = 0usize;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index)?;
        if !file.is_file() {
            continue;
        }
        let enclosed = file
            .enclosed_name()
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let relative = enclosed
            .split_once('/')
            .map(|(_, rest)| rest.to_owned())
            .unwrap_or(enclosed);
        let Some(path) = sanitize_relative_path(&relative) else {
            continue;
        };
        if should_skip(&path) {
            continue;
        }
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        total = total.saturating_add(content.len());
        if total > MAX_SITE_BYTES {
            bail!("the GitHub site exceeds the 25 MiB HTML page limit");
        }
        files.push((path, content));
        if files.len() > MAX_FILES {
            bail!("the GitHub site has too many files for HTML page hosting");
        }
    }
    Ok(files)
}

pub fn materialize_github_files(
    files: Vec<(String, Vec<u8>)>,
    collect_origin: &str,
    site_id: Uuid,
) -> Result<Vec<HtmlFile>, AppError> {
    let paths = files.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>();
    if !paths.iter().any(|path| path.ends_with("index.html")) {
        return Err(AppError::BadRequest {
            code: "HTML_INDEX_MISSING",
            message: "The repository must contain an index.html file to host as a page.",
        });
    }
    let root = detect_site_root(&paths);
    let mut out = Vec::new();
    for (path, content) in files {
        let relative = if root.is_empty() {
            path
        } else {
            match path.strip_prefix(&format!("{root}/")) {
                Some(rest) => rest.to_owned(),
                None => continue,
            }
        };
        let Some(clean) = sanitize_relative_path(&relative) else {
            continue;
        };
        let mut body = content;
        if clean.ends_with(".html") || clean.ends_with(".htm") {
            if let Ok(html) = String::from_utf8(body.clone()) {
                body = inject_analytics(&html, collect_origin, site_id).into_bytes();
            }
        }
        out.push(HtmlFile {
            content_type: content_type_for(&clean),
            path: clean,
            content: body,
        });
    }
    if !out.iter().any(|file| file.path == "index.html") {
        return Err(AppError::BadRequest {
            code: "HTML_INDEX_MISSING",
            message: "The repository must contain an index.html file to host as a page.",
        });
    }
    Ok(out)
}

pub async fn replace_files(
    db: &sqlx::PgPool,
    service_id: Uuid,
    files: &[HtmlFile],
) -> Result<(), sqlx::Error> {
    let mut transaction = db.begin().await?;
    sqlx::query("DELETE FROM html_page_files WHERE app_service_id = $1")
        .bind(service_id)
        .execute(&mut *transaction)
        .await?;
    for file in files {
        sqlx::query(
            "INSERT INTO html_page_files (app_service_id, path, content, content_type) VALUES ($1, $2, $3, $4)",
        )
        .bind(service_id)
        .bind(&file.path)
        .bind(&file.content)
        .bind(&file.content_type)
        .execute(&mut *transaction)
        .await?;
    }
    transaction.commit().await?;
    Ok(())
}

pub async fn write_site_to_disk(
    data_dir: &str,
    service_id: Uuid,
    files: &[HtmlFile],
) -> Result<()> {
    let root = site_dir(data_dir, service_id);
    let html_root = root.join("html");
    if html_root.exists() {
        tokio::fs::remove_dir_all(&html_root).await?;
    }
    tokio::fs::create_dir_all(&html_root).await?;
    for file in files {
        let dest = html_root.join(&file.path);
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(dest, &file.content).await?;
    }
    tokio::fs::write(root.join("default.conf"), nginx_conf()).await?;
    Ok(())
}

pub async fn load_files(
    db: &sqlx::PgPool,
    service_id: Uuid,
) -> Result<Vec<HtmlFile>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT path, content, content_type FROM html_page_files WHERE app_service_id = $1",
    )
    .bind(service_id)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| HtmlFile {
            path: row.get("path"),
            content: row.get("content"),
            content_type: row.get("content_type"),
        })
        .collect())
}

pub fn collect_origin(config: &crate::config::Config) -> String {
    config.mcp_public_base_url.trim_end_matches('/').to_owned()
}

pub async fn fetch_github_commit_sha(
    token: &str,
    owner: &str,
    repo: &str,
    branch: Option<&str>,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("knotree-cloud")
        .build()?;
    let repo_url = format!("https://api.github.com/repos/{owner}/{repo}");
    let repo_response = client
        .get(&repo_url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    if repo_response.status() == StatusCode::NOT_FOUND {
        bail!("GitHub repository {owner}/{repo} was not found or the connected account cannot read it");
    }
    if !repo_response.status().is_success() {
        bail!(
            "GitHub repository lookup returned HTTP {}",
            repo_response.status()
        );
    }
    let repo_json = repo_response.json::<serde_json::Value>().await?;
    let default_branch = repo_json
        .get("default_branch")
        .and_then(|value| value.as_str())
        .unwrap_or("main")
        .to_owned();
    let ref_name = branch
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(default_branch.as_str())
        .to_owned();
    let commit_url = format!("https://api.github.com/repos/{owner}/{repo}/commits/{ref_name}");
    let commit_response = client
        .get(&commit_url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    if !commit_response.status().is_success() {
        bail!(
            "GitHub commit lookup returned HTTP {}",
            commit_response.status()
        );
    }
    let commit_json = commit_response.json::<serde_json::Value>().await?;
    commit_json
        .get("sha")
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned)
        .context("GitHub did not return a commit SHA")
}

pub async fn fetch_github_site(
    token: &str,
    owner: &str,
    repo: &str,
    branch: Option<&str>,
) -> Result<(String, Vec<(String, Vec<u8>)>)> {
    let sha = fetch_github_commit_sha(token, owner, repo, branch).await?;
    let client = reqwest::Client::builder()
        .user_agent("knotree-cloud")
        .build()?;
    let zip_url = format!("https://api.github.com/repos/{owner}/{repo}/zipball/{sha}");
    let zip_response = client
        .get(&zip_url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await?;
    if !zip_response.status().is_success() {
        bail!(
            "GitHub zipball download returned HTTP {}",
            zip_response.status()
        );
    }
    let bytes = zip_response.bytes().await?;
    Ok((sha, unzip_github_pages(&bytes)?))
}

pub async fn public_cors(request: Request<Body>, next: Next) -> Response {
    let is_public_html = request
        .uri()
        .path()
        .contains("/api/v1/public/html-pages/");
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .cloned()
        .unwrap_or_else(|| HeaderValue::from_static("*"));
    if is_public_html && request.method() == Method::OPTIONS {
        let mut response = StatusCode::NO_CONTENT.into_response();
        apply_public_cors(response.headers_mut(), &origin);
        return response;
    }
    let mut response = next.run(request).await;
    if is_public_html {
        apply_public_cors(response.headers_mut(), &origin);
    }
    response
}

fn apply_public_cors(headers: &mut HeaderMap, origin: &HeaderValue) {
    headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
}

pub async fn analytics_script(
    State(state): State<AppState>,
    AxumPath(app_service_id): AxumPath<Uuid>,
) -> Result<Response, AppError> {
    ensure_html_service(&state, app_service_id).await?;
    let body = analytics_javascript(&collect_origin(&state.config), app_service_id);
    Ok((
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "public, max-age=3600, stale-while-revalidate=86400",
            ),
        ],
        body,
    )
        .into_response())
}

pub async fn collect_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(app_service_id): AxumPath<Uuid>,
    Json(input): Json<HtmlAnalyticsEventRequest>,
) -> Result<StatusCode, AppError> {
    ensure_html_service(&state, app_service_id).await?;
    let event_type = input
        .event_type
        .unwrap_or_else(|| "pageview".to_owned())
        .chars()
        .take(40)
        .collect::<String>();
    let path = input
        .path
        .unwrap_or_else(|| "/".to_owned())
        .chars()
        .take(2048)
        .collect::<String>();
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.chars().take(400).collect::<String>());
    sqlx::query(
        "INSERT INTO html_page_events (id, app_service_id, event_type, path, referrer, language, timezone, screen_width, screen_height, viewport_width, viewport_height, user_agent, session_id, duration_ms, extra)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(Uuid::new_v4())
    .bind(app_service_id)
    .bind(event_type)
    .bind(path)
    .bind(input.referrer.map(|value| value.chars().take(2048).collect::<String>()))
    .bind(input.language.map(|value| value.chars().take(32).collect::<String>()))
    .bind(input.timezone.map(|value| value.chars().take(64).collect::<String>()))
    .bind(input.screen_width)
    .bind(input.screen_height)
    .bind(input.viewport_width)
    .bind(input.viewport_height)
    .bind(user_agent)
    .bind(input.session_id.map(|value| value.chars().take(80).collect::<String>()))
    .bind(input.duration_ms)
    .bind(input.extra.unwrap_or_else(|| serde_json::json!({})))
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn analytics_summary(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((workspace_slug, project_slug, app_service_id)): AxumPath<(String, String, Uuid)>,
) -> Result<Json<HtmlAnalyticsSummary>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let source = sqlx::query_scalar::<_, String>(
        "SELECT image_source FROM project_app_services WHERE id = $1 AND project_id = $2",
    )
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;
    if !is_html_source(&source) {
        return Err(AppError::BadRequest {
            code: "NOT_HTML_PAGE",
            message: "Analytics are available for HTML pages only.",
        });
    }
    Ok(Json(load_summary(&state, app_service_id).await?))
}

pub async fn get_index_html(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((workspace_slug, project_slug, app_service_id)): AxumPath<(String, String, Uuid)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project_id =
        projects::accessible_project_id(&state, user.id, &workspace_slug, &project_slug).await?;
    let row = sqlx::query(
        "SELECT image_source FROM project_app_services WHERE id = $1 AND project_id = $2",
    )
    .bind(app_service_id)
    .bind(project_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;
    let source: String = row.get("image_source");
    if source != IMAGE_SOURCE_HTML {
        return Err(AppError::BadRequest {
            code: "HTML_PASTE_ONLY",
            message: "Only pasted HTML pages can be edited in the dashboard.",
        });
    }
    let content = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT content FROM html_page_files WHERE app_service_id = $1 AND path = 'index.html'",
    )
    .bind(app_service_id)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or_default();
    let index_html = String::from_utf8(content).unwrap_or_default();
    Ok(Json(serde_json::json!({ "indexHtml": index_html })))
}

async fn ensure_html_service(state: &AppState, app_service_id: Uuid) -> Result<(), AppError> {
    let source = sqlx::query_scalar::<_, String>(
        "SELECT image_source FROM project_app_services WHERE id = $1",
    )
    .bind(app_service_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "APP_SERVICE_NOT_FOUND",
        message: "The app service could not be found.",
    })?;
    if !is_html_source(&source) {
        return Err(AppError::NotFound {
            code: "APP_SERVICE_NOT_FOUND",
            message: "The app service could not be found.",
        });
    }
    Ok(())
}

async fn load_summary(
    state: &AppState,
    app_service_id: Uuid,
) -> Result<HtmlAnalyticsSummary, AppError> {
    let pageviews = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM html_page_events WHERE app_service_id = $1 AND event_type = 'pageview'",
    )
    .bind(app_service_id)
    .fetch_one(&state.db)
    .await?;
    let sessions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(DISTINCT session_id) FROM html_page_events WHERE app_service_id = $1 AND session_id IS NOT NULL",
    )
    .bind(app_service_id)
    .fetch_one(&state.db)
    .await?;
    let avg_duration_ms = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(duration_ms)::float8 FROM html_page_events WHERE app_service_id = $1 AND duration_ms IS NOT NULL",
    )
    .bind(app_service_id)
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0.0);
    let top_paths = count_rows(
        state,
        "SELECT path AS name, COUNT(*) AS count FROM html_page_events WHERE app_service_id = $1 AND event_type = 'pageview' GROUP BY path ORDER BY count DESC LIMIT 10",
        app_service_id,
    )
    .await?;
    let top_referrers = count_rows(
        state,
        "SELECT COALESCE(NULLIF(referrer, ''), '(direct)') AS name, COUNT(*) AS count FROM html_page_events WHERE app_service_id = $1 AND event_type = 'pageview' GROUP BY 1 ORDER BY count DESC LIMIT 10",
        app_service_id,
    )
    .await?;
    let browsers = count_rows(
        state,
        "SELECT CASE
            WHEN user_agent ILIKE '%Firefox%' THEN 'Firefox'
            WHEN user_agent ILIKE '%Edg%' THEN 'Edge'
            WHEN user_agent ILIKE '%Chrome%' THEN 'Chrome'
            WHEN user_agent ILIKE '%Safari%' THEN 'Safari'
            ELSE COALESCE(NULLIF(user_agent, ''), 'unknown')
         END AS name, COUNT(*) AS count
         FROM html_page_events WHERE app_service_id = $1 AND event_type = 'pageview' GROUP BY 1 ORDER BY count DESC LIMIT 8",
        app_service_id,
    )
    .await?;
    let event_types = count_rows(
        state,
        "SELECT event_type AS name, COUNT(*) AS count FROM html_page_events WHERE app_service_id = $1 GROUP BY event_type ORDER BY count DESC",
        app_service_id,
    )
    .await?;
    let recent_rows = sqlx::query(
        "SELECT occurred_at, event_type, path, referrer, session_id FROM html_page_events WHERE app_service_id = $1 ORDER BY occurred_at DESC LIMIT 25",
    )
    .bind(app_service_id)
    .fetch_all(&state.db)
    .await?;
    let recent = recent_rows
        .into_iter()
        .map(|row| {
            let occurred_at: OffsetDateTime = row.get("occurred_at");
            HtmlRecentEvent {
                occurred_at: occurred_at
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default(),
                event_type: row.get("event_type"),
                path: row.get("path"),
                referrer: row.get("referrer"),
                session_id: row.get("session_id"),
            }
        })
        .collect();
    Ok(HtmlAnalyticsSummary {
        pageviews,
        sessions,
        avg_duration_ms,
        top_paths,
        top_referrers,
        browsers,
        event_types,
        recent,
    })
}

async fn count_rows(
    state: &AppState,
    sql: &str,
    app_service_id: Uuid,
) -> Result<Vec<HtmlCountRow>, AppError> {
    let rows = sqlx::query(sql)
        .bind(app_service_id)
        .fetch_all(&state.db)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| HtmlCountRow {
            name: row.get("name"),
            count: row.get("count"),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_unique_page_domains() {
        assert_eq!(page_subdomain("docs").unwrap(), "page-docs");
        assert_eq!(page_subdomain("page-docs").unwrap(), "page-docs");
        assert!(page_subdomain("Docs_Site").is_err());
        assert!(page_subdomain("").is_err());
    }

    #[test]
    fn injects_analytics_into_head() {
        let html = "<html><head><title>Hi</title></head><body>ok</body></html>";
        let id = Uuid::nil();
        let out = inject_analytics(html, "https://api.example", id);
        assert!(out.contains("</script>\n</head>"));
        assert!(out.contains("/analytics.js"));
    }

    #[test]
    fn detects_docs_root_like_github_pages() {
        let paths = vec![
            "README.md".to_owned(),
            "docs/index.html".to_owned(),
            "docs/style.css".to_owned(),
        ];
        assert_eq!(detect_site_root(&paths), "docs");
    }

    #[test]
    fn parses_github_urls() {
        assert_eq!(
            parse_github_repo("https://github.com/acme/site.git").unwrap(),
            ("acme".to_owned(), "site".to_owned())
        );
        assert_eq!(
            parse_github_repo("acme/site").unwrap(),
            ("acme".to_owned(), "site".to_owned())
        );
        assert!(parse_github_repo("not a repo").is_err());
    }
}
