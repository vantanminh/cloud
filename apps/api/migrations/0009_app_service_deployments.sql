CREATE TABLE app_service_deployments (
    id UUID PRIMARY KEY,
    app_service_id UUID NOT NULL REFERENCES project_app_services(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('provisioning', 'ready', 'error')),
    current_step TEXT NOT NULL,
    logs TEXT NOT NULL DEFAULT '',
    error_message TEXT,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX app_service_deployments_service_started_idx
    ON app_service_deployments (app_service_id, started_at DESC);
