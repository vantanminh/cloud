ALTER TABLE project_app_services
    ADD COLUMN database_resource_id UUID
        REFERENCES project_postgres_databases(id) ON DELETE SET NULL;

CREATE INDEX project_app_services_database_resource_id_idx
    ON project_app_services (database_resource_id);
