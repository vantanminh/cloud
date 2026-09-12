CREATE TABLE project_postgres_databases (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    database_name TEXT NOT NULL CHECK (database_name ~ '^[a-z0-9_]+$'),
    role_name TEXT NOT NULL CHECK (role_name ~ '^[a-z0-9_]+$'),
    host TEXT NOT NULL CHECK (char_length(host) BETWEEN 1 AND 255),
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    password_ciphertext TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'provisioning' CHECK (status IN ('provisioning', 'ready', 'error')),
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_postgres_databases_project_id_key UNIQUE (project_id),
    CONSTRAINT project_postgres_databases_database_name_key UNIQUE (database_name),
    CONSTRAINT project_postgres_databases_role_name_key UNIQUE (role_name)
);

CREATE INDEX project_postgres_databases_status_idx
    ON project_postgres_databases (status);
