CREATE TABLE knotree_registry_connections (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    registry_host TEXT NOT NULL DEFAULT 'registry.knotree.com'
        CHECK (registry_host = 'registry.knotree.com'),
    registry_username TEXT NOT NULL CHECK (char_length(registry_username) BETWEEN 1 AND 128),
    repository TEXT NOT NULL CHECK (char_length(repository) BETWEEN 1 AND 255),
    credential_ciphertext TEXT NOT NULL,
    verified_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX knotree_registry_connections_project_idx
    ON knotree_registry_connections (project_id, created_at DESC);

ALTER TABLE project_app_services
    ADD COLUMN registry_connection_id UUID
        REFERENCES knotree_registry_connections(id) ON DELETE SET NULL;

ALTER TABLE project_app_services
    DROP CONSTRAINT IF EXISTS project_app_services_image_source_check;

ALTER TABLE project_app_services
    ADD CONSTRAINT project_app_services_image_source_check
    CHECK (image_source IN ('public', 'github', 'html', 'html_github', 'knotree_registry')),
    ADD CONSTRAINT project_app_services_registry_connection_check
        CHECK (registry_connection_id IS NULL OR image_source = 'knotree_registry');

CREATE INDEX project_app_services_registry_connection_idx
    ON project_app_services (registry_connection_id)
    WHERE registry_connection_id IS NOT NULL;

CREATE TABLE knotree_registry_webhook_deliveries (
    delivery_id UUID PRIMARY KEY,
    event_kind TEXT NOT NULL CHECK (event_kind = 'tag_updated'),
    received_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE knotree_registry_deploy_jobs (
    id UUID PRIMARY KEY,
    delivery_id UUID NOT NULL
        REFERENCES knotree_registry_webhook_deliveries(delivery_id) ON DELETE CASCADE,
    app_service_id UUID NOT NULL REFERENCES project_app_services(id) ON DELETE CASCADE,
    image_digest TEXT NOT NULL CHECK (image_digest ~ '^sha256:[0-9a-f]{64}$'),
    immutable_image TEXT NOT NULL CHECK (char_length(immutable_image) BETWEEN 1 AND 512),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    locked_until TIMESTAMPTZ,
    deployment_id UUID REFERENCES app_service_deployments(id) ON DELETE SET NULL,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT knotree_registry_deploy_jobs_delivery_service_key
        UNIQUE (delivery_id, app_service_id)
);

CREATE INDEX knotree_registry_deploy_jobs_pending_idx
    ON knotree_registry_deploy_jobs (status, created_at)
    WHERE status IN ('pending', 'running');
