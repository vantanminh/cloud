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
    models::{CreateWorkspaceRequest, WorkspaceResponse},
    security,
    state::AppState,
};

#[derive(Debug, sqlx::FromRow)]
struct WorkspaceRow {
    id: Uuid,
    name: String,
    slug: String,
}

pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateWorkspaceRequest>,
) -> Result<Response, AppError> {
    security::require_csrf(&headers, &state.config)?;
    let user = auth::authenticate(&state, &headers).await?;
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 80 || name.chars().any(char::is_control) {
        let mut fields = BTreeMap::new();
        fields.insert(
            "name".to_owned(),
            "Enter a name between 1 and 80 characters.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }

    let slug = normalize_slug(input.slug.as_deref().unwrap_or(&name));
    if slug.is_empty() || slug.chars().count() > 48 || is_reserved_slug(&slug) {
        let mut fields = BTreeMap::new();
        fields.insert(
            "slug".to_owned(),
            "Use a unique workspace URL slug.".to_owned(),
        );
        return Err(AppError::validation(fields));
    }

    let mut transaction = state.db.begin().await?;
    let workspace_id = Uuid::new_v4();
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "INSERT INTO workspaces (id, name, slug) VALUES ($1, $2, $3) RETURNING id, name, slug",
    )
    .bind(workspace_id)
    .bind(&name)
    .bind(&slug)
    .fetch_one(&mut *transaction)
    .await;

    let workspace = match workspace {
        Ok(workspace) => workspace,
        Err(error) => {
            if unique_constraint(&error) == Some("workspaces_slug_key") {
                return Err(AppError::Conflict {
                    code: "SLUG_TAKEN",
                    message: "That workspace URL is already in use.",
                });
            }
            return Err(error.into());
        }
    };

    let membership_result = sqlx::query(
        "INSERT INTO workspace_memberships (workspace_id, user_id, role) VALUES ($1, $2, 'owner')",
    )
    .bind(workspace.id)
    .bind(user.id)
    .execute(&mut *transaction)
    .await;
    if let Err(error) = membership_result {
        if unique_constraint(&error) == Some("workspace_memberships_user_id_key") {
            return Err(AppError::Conflict {
                code: "WORKSPACE_EXISTS",
                message: "This account already has a workspace.",
            });
        }
        return Err(error.into());
    }
    transaction.commit().await?;

    Ok((
        StatusCode::CREATED,
        Json(WorkspaceResponse {
            id: workspace.id,
            name: workspace.name,
            slug: workspace.slug,
        }),
    )
        .into_response())
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(slug): Path<String>,
) -> Result<Json<WorkspaceResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT w.id, w.name, w.slug FROM workspaces w INNER JOIN workspace_memberships wm ON wm.workspace_id = w.id WHERE wm.user_id = $1 AND w.slug = $2",
    )
    .bind(user.id)
    .bind(slug)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "WORKSPACE_NOT_FOUND",
        message: "Workspace not found.",
    })?;

    Ok(Json(WorkspaceResponse {
        id: workspace.id,
        name: workspace.name,
        slug: workspace.slug,
    }))
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

fn is_reserved_slug(slug: &str) -> bool {
    matches!(slug, "login" | "register" | "new" | "workspace")
}

fn unique_constraint(error: &sqlx::Error) -> Option<&str> {
    error
        .as_database_error()
        .and_then(|database_error| database_error.constraint())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_stable_kebab_case_slug() {
        assert_eq!(normalize_slug("  Cà phê Studio  "), "ca-phe-studio");
    }

    #[test]
    fn rejects_reserved_routes() {
        assert!(is_reserved_slug("login"));
        assert!(!is_reserved_slug("acme"));
    }
}
