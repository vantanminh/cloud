use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
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

    let mut transaction = state.db.begin().await?;
    let workspace_id = Uuid::new_v4();
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "INSERT INTO workspaces (id, name) VALUES ($1, $2) RETURNING id, name",
    )
    .bind(workspace_id)
    .bind(&name)
    .fetch_one(&mut *transaction)
    .await;

    let workspace = workspace?;

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
        }),
    )
        .into_response())
}

pub async fn get(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(workspace_id): Path<Uuid>,
) -> Result<Json<WorkspaceResponse>, AppError> {
    let user = auth::authenticate(&state, &headers).await?;
    let workspace = sqlx::query_as::<_, WorkspaceRow>(
        "SELECT w.id, w.name FROM workspaces w INNER JOIN workspace_memberships wm ON wm.workspace_id = w.id WHERE wm.user_id = $1 AND w.id = $2",
    )
    .bind(user.id)
    .bind(workspace_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound {
        code: "WORKSPACE_NOT_FOUND",
        message: "Workspace not found.",
    })?;

    Ok(Json(WorkspaceResponse {
        id: workspace.id,
        name: workspace.name,
    }))
}

fn unique_constraint(error: &sqlx::Error) -> Option<&str> {
    error
        .as_database_error()
        .and_then(|database_error| database_error.constraint())
}
