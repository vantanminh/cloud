ALTER TABLE project_postgres_databases
    ADD COLUMN cluster_provider TEXT NOT NULL DEFAULT 'legacy_shared',
    ADD COLUMN cluster_name TEXT,
    ADD COLUMN cluster_namespace TEXT,
    ADD COLUMN cluster_volume TEXT,
    ADD COLUMN cluster_host TEXT,
    ADD COLUMN cluster_port INTEGER,
    ADD COLUMN public_host TEXT,
    ADD COLUMN public_port INTEGER;

ALTER TABLE project_postgres_databases
    ADD CONSTRAINT project_postgres_databases_cluster_provider_check
    CHECK (cluster_provider IN ('docker', 'kubernetes', 'legacy_shared'));

ALTER TABLE project_postgres_databases
    ADD CONSTRAINT project_postgres_databases_cluster_port_check
    CHECK (cluster_port IS NULL OR cluster_port BETWEEN 1 AND 65535);

ALTER TABLE project_postgres_databases
    ADD CONSTRAINT project_postgres_databases_public_port_check
    CHECK (public_port IS NULL OR public_port BETWEEN 1 AND 65535);

CREATE INDEX project_postgres_databases_cluster_provider_idx
    ON project_postgres_databases (cluster_provider);
