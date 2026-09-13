ALTER TABLE project_app_services
    DROP CONSTRAINT IF EXISTS project_app_services_project_id_key;

CREATE INDEX project_app_services_project_id_created_at_idx
    ON project_app_services (project_id, created_at, id);
