use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;
use tokio::sync::OnceCell;
use uuid::Uuid;

use crate::{config::Config, state::AppState};

static MIGRATIONS: OnceCell<()> = OnceCell::const_new();

#[allow(dead_code)]
pub struct SeededProject {
    pub user_id: Uuid,
    pub workspace_id: Uuid,
    pub workspace_route_id: String,
    pub project_id: Uuid,
    pub project_slug: String,
}

pub async fn test_app_state() -> Option<AppState> {
    test_app_state_configured(|_| {}).await
}

pub async fn test_app_state_configured(configure: impl FnOnce(&mut Config)) -> Option<AppState> {
    let url = match std::env::var("DATABASE_URL") {
        Ok(url) if !url.trim().is_empty() => url,
        _ if std::env::var("CI").is_ok() => {
            panic!("DATABASE_URL must be set in CI for API integration tests")
        }
        _ => return None,
    };
    let pool = match PgPoolOptions::new().max_connections(8).connect(&url).await {
        Ok(pool) => pool,
        Err(error) if std::env::var("CI").is_ok() => {
            panic!("could not connect to DATABASE_URL in CI: {error}")
        }
        Err(_) => return None,
    };
    let migrator = pool.clone();
    MIGRATIONS
        .get_or_init(|| async move {
            sqlx::migrate!("./migrations")
                .run(&migrator)
                .await
                .expect("test database migrations should apply");
        })
        .await;

    let mut config = Config::test_fixture();
    config.database_url = url;
    config.docs_dir = format!("{}/../../docs", env!("CARGO_MANIFEST_DIR"));
    config.kong_admin_url = None;
    config.app_service_provisioning_enabled = true;
    config.database_provisioning_enabled = false;
    configure(&mut config);
    Some(AppState::new(pool, Arc::new(config)))
}

pub async fn seed_owner_project(state: &AppState) -> SeededProject {
    let user_id = Uuid::new_v4();
    let workspace_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let suffix = user_id.simple().to_string();
    let workspace_route_id = workspace_id.to_string();
    let project_slug = format!("proj-{suffix}");
    sqlx::query("INSERT INTO users (id, full_name, email, password_hash) VALUES ($1, $2, $3, $4)")
        .bind(user_id)
        .bind("Test Owner")
        .bind(format!("owner-{suffix}@example.com"))
        .bind("test-password-hash")
        .execute(&state.db)
        .await
        .expect("seed user");
    sqlx::query("INSERT INTO workspaces (id, name) VALUES ($1, $2)")
        .bind(workspace_id)
        .bind("Test Workspace")
        .execute(&state.db)
        .await
        .expect("seed workspace");
    sqlx::query(
        "INSERT INTO workspace_memberships (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(workspace_id)
    .bind(user_id)
    .execute(&state.db)
    .await
    .expect("seed membership");
    sqlx::query("INSERT INTO projects (id, workspace_id, name, slug) VALUES ($1, $2, $3, $4)")
        .bind(project_id)
        .bind(workspace_id)
        .bind("Test Project")
        .bind(&project_slug)
        .execute(&state.db)
        .await
        .expect("seed project");
    SeededProject {
        user_id,
        workspace_id,
        workspace_route_id,
        project_id,
        project_slug,
    }
}

pub async fn session_headers(state: &AppState, user_id: Uuid) -> axum::http::HeaderMap {
    use axum::http::{HeaderMap, HeaderValue, header};
    use time::{Duration, OffsetDateTime};

    let token = crate::security::random_token();
    sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(crate::security::token_hash(&token))
        .bind(user_id)
        .bind(OffsetDateTime::now_utc() + Duration::days(state.config.session_ttl_days))
        .execute(&state.db)
        .await
        .expect("seed session");
    let csrf = "test-csrf-token";
    let mut headers = HeaderMap::new();
    headers.insert(
        header::COOKIE,
        HeaderValue::from_str(&format!(
            "{}={token}; {}={csrf}",
            state.config.session_cookie_name(),
            state.config.csrf_cookie_name()
        ))
        .expect("session cookie"),
    );
    headers.insert(
        header::ORIGIN,
        HeaderValue::from_static("http://localhost:5173"),
    );
    headers.insert("x-csrf-token", HeaderValue::from_static(csrf));
    headers
}
