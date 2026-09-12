use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use deunicode::deunicode;
use uuid::Uuid;

use crate::{
    auth,
    error::AppError,
    models::{CreateProjectRequest, ProjectResponse},
    security,
    state::AppState,
};

#[derive(Debug, sqlx::FromRow)]
struct ProjectRow {
    id: Uuid,
    name: String,
    slug: String,
}

pub async fn list(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_slug): Path<String>,
) -> Result<Json<Vec<ProjectResponse>>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let workspace_id = accessible_workspace_id(&state, user.id, &workspace_slug).await?;
    let projects = sqlx::query_as::<_, ProjectRow>(
        "SELECT id, name, slug FROM projects WHERE workspace_id = $1 ORDER BY created_at ASC, id ASC",
    )
    .bind(workspace_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(
        projects.into_iter().map(ProjectResponse::from).collect(),
    ))
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_slug): Path<String>,
    Json(input): Json<CreateProjectRequest>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let workspace_id = accessible_workspace_id(&state, user.id, &workspace_slug).await?;

    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        let mut fields = BTreeMap::new();
        fields.insert(
            "name".to_owned(),
            "Enter a project name between 1 and 80 characters.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }

    let slug = normalize_slug(input.slug.as_deref().unwrap_or(&name));
    if slug.is_empty() || slug.chars().count() > 48 {
        let mut fields = BTreeMap::new();
        fields.insert(
            "slug".to_owned(),
            "Use a lowercase project URL slug.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }

    let project_id = Uuid::new_v4();
    let project = sqlx::query_as::<_, ProjectRow>(
        "INSERT INTO projects (id, workspace_id, name, slug) VALUES ($1, $2, $3, $4) RETURNING id, name, slug",
    )
    .bind(project_id)
    .bind(workspace_id)
    .bind(&name)
    .bind(&slug)
    .fetch_one(&state.db)
    .await;

    match project {
        Ok(project) => {
            Ok((StatusCode::CREATED, Json(ProjectResponse::from(project))).into_response())
        }
        Err(error) if unique_constraint(&error) == Some("projects_workspace_slug_key") => {
            Err(AppError::Conflict {
                code: "PROJECT_SLUG_TAKEN",
                message: "That project URL is already in use in this workspace.",
            })
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((workspace_slug, project_slug)): Path<(String, String)>,
) -> Result<Json<ProjectResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let project = sqlx::query_as::<_, ProjectRow>(
        "SELECT p.id, p.name, p.slug FROM projects p INNER JOIN workspaces w ON w.id = p.workspace_id INNER JOIN workspace_memberships wm ON wm.workspace_id = w.id WHERE wm.user_id = $1 AND w.slug = $2 AND p.slug = $3",
    )
    .bind(user.id)
    .bind(workspace_slug)
    .bind(project_slug)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "PROJECT_NOT_FOUND",
        message: "Project not found.",
    })?;

    Ok(Json(ProjectResponse::from(project)))
}

async fn accessible_workspace_id(
    state: &AppState,
    user_id: Uuid,
    workspace_slug: &str,
) -> Result<Uuid, AppError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT w.id FROM workspaces w INNER JOIN workspace_memberships wm ON wm.workspace_id = w.id WHERE wm.user_id = $1 AND w.slug = $2",
    )
    .bind(user_id)
    .bind(workspace_slug)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "WORKSPACE_NOT_FOUND",
        message: "Workspace not found.",
    })
}

fn normalize_slug(value: &str) -> String {
    let transliterated = deunicode(value).to_lowercase();
    let mut slug = String::new();
    let mut pending_separator = false;
    for character in transliterated.chars() {
        if character.is_ascii_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            pending_separator = false;
        } else if !slug.is_empty() {
            pending_separator = true;
        }
    }
    slug
}

fn unique_constraint(error: &sqlx::Error) -> Option<&str> {
    error
        .as_database_error()
        .and_then(|database_error| database_error.constraint())
}

impl From<ProjectRow> for ProjectResponse {
    fn from(project: ProjectRow) -> Self {
        Self {
            id: project.id,
            name: project.name,
            slug: project.slug,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_stable_kebab_case_project_slug() {
        assert_eq!(normalize_slug("  Cà phê API  "), "ca-phe-api");
    }

    #[test]
    fn removes_leading_and_trailing_separators() {
        assert_eq!(normalize_slug("--cloud--project--"), "cloud-project");
    }
}
