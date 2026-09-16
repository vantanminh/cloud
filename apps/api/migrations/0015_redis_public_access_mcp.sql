ALTER TABLE project_app_services
    ALTER COLUMN public_subdomain DROP NOT NULL;

ALTER TABLE project_app_services
    ADD COLUMN public_access_enabled BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN rate_limit_rpm INTEGER NOT NULL DEFAULT 60;

UPDATE project_app_services
SET public_access_enabled = true
WHERE public_subdomain IS NOT NULL AND btrim(public_subdomain) <> '';

ALTER TABLE project_app_services
    ADD CONSTRAINT project_app_services_rate_limit_rpm_check
        CHECK (rate_limit_rpm BETWEEN 1 AND 10000);

CREATE TABLE project_redis_instances (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 80),
    password_ciphertext TEXT NOT NULL,
    host TEXT NOT NULL CHECK (char_length(host) BETWEEN 1 AND 255),
    port INTEGER NOT NULL CHECK (port BETWEEN 1 AND 65535),
    status TEXT NOT NULL DEFAULT 'provisioning'
        CHECK (status IN ('provisioning', 'ready', 'error')),
    error_message TEXT,
    cluster_provider TEXT NOT NULL,
    cluster_name TEXT,
    cluster_namespace TEXT,
    cluster_volume TEXT,
    public_host TEXT,
    public_port INTEGER,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT project_redis_instances_project_id_key UNIQUE (project_id)
);

CREATE INDEX project_redis_instances_status_idx
    ON project_redis_instances (status);

CREATE TABLE redis_metric_samples (
    resource_id UUID NOT NULL REFERENCES project_redis_instances(id) ON DELETE CASCADE,
    sampled_at TIMESTAMPTZ NOT NULL,
    cpu_percent DOUBLE PRECISION,
    memory_used_bytes BIGINT,
    memory_limit_bytes BIGINT,
    volume_used_bytes BIGINT,
    volume_capacity_bytes BIGINT,
    network_receive_bytes BIGINT,
    network_transmit_bytes BIGINT,
    disk_read_bytes BIGINT,
    disk_write_bytes BIGINT,
    PRIMARY KEY (resource_id, sampled_at)
);

CREATE INDEX redis_metric_samples_resource_sampled_at_idx
    ON redis_metric_samples (resource_id, sampled_at DESC);

CREATE TABLE mcp_oauth_clients (
    id UUID PRIMARY KEY,
    client_id TEXT NOT NULL UNIQUE,
    client_secret_hash BYTEA,
    redirect_uris TEXT[] NOT NULL,
    name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE mcp_oauth_codes (
    code_hash BYTEA PRIMARY KEY,
    client_id TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    redirect_uri TEXT NOT NULL,
    code_challenge TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX mcp_oauth_codes_expires_at_idx
    ON mcp_oauth_codes (expires_at);

CREATE TABLE mcp_oauth_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    client_id TEXT NOT NULL,
    access_token_hash BYTEA NOT NULL UNIQUE,
    refresh_token_hash BYTEA NOT NULL UNIQUE,
    access_expires_at TIMESTAMPTZ NOT NULL,
    refresh_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX mcp_oauth_tokens_user_id_idx
    ON mcp_oauth_tokens (user_id, created_at DESC);
