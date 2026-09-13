ALTER TABLE project_app_services
    ADD COLUMN auto_deploy_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN github_connection_user_id UUID REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN deployed_image_digest TEXT,
    ADD COLUMN auto_deploy_checked_at TIMESTAMPTZ,
    ADD COLUMN auto_deploy_error TEXT;

UPDATE project_app_services AS service
SET auto_deploy_enabled = EXISTS (
        SELECT 1
        FROM github_connections AS connection
        JOIN workspace_memberships AS membership
          ON membership.workspace_id = project.workspace_id
         AND membership.user_id = connection.user_id
    ),
    github_connection_user_id = (
        SELECT connection.user_id
        FROM github_connections AS connection
        JOIN workspace_memberships AS membership
          ON membership.workspace_id = project.workspace_id
         AND membership.user_id = connection.user_id
        ORDER BY connection.updated_at DESC
        LIMIT 1
    )
FROM projects AS project
WHERE service.project_id = project.id
  AND service.image_source = 'github';

CREATE INDEX project_app_services_auto_deploy_idx
    ON project_app_services (status, auto_deploy_checked_at)
    WHERE auto_deploy_enabled = TRUE;
