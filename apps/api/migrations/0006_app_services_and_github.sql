CREATE TABLE project_app_services (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    image TEXT NOT NULL CHECK (char_length(image) BETWEEN 1 AND 255),
    image_source TEXT NOT NULL CHECK (image_source IN ('public', 'github')),
    app_port INTEGER NOT NULL DEFAULT 3000 CHECK (app_port BETWEEN 1 AND 65535),
    host TEXT,
    port INTEGER CHECK (port IS NULL OR port BETWEEN 1 AND 65535),
    container_name TEXT UNIQUE,
    status TEXT NOT NULL DEFAULT 'provisioning'
        CHECK (status IN ('provisioning', 'ready', 'error')),
    error_message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_app_services_project_id_key UNIQUE (project_id)
);

CREATE INDEX project_app_services_status_idx
    ON project_app_services (status);

CREATE TABLE github_connections (
    user_id UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    github_user_id TEXT NOT NULL,
    github_login TEXT NOT NULL,
    access_token_ciphertext TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE github_oauth_states (
    state_hash BYTEA PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    return_to TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX github_oauth_states_expires_at_idx
    ON github_oauth_states (expires_at);
