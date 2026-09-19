ALTER TABLE project_app_services
    DROP CONSTRAINT IF EXISTS project_app_services_image_source_check;

ALTER TABLE project_app_services
    ADD CONSTRAINT project_app_services_image_source_check
        CHECK (image_source IN ('public', 'github', 'html', 'html_github'));

ALTER TABLE project_app_services
    ADD COLUMN html_repo TEXT,
    ADD COLUMN html_branch TEXT,
    ADD COLUMN html_sha TEXT;

CREATE TABLE html_page_files (
    app_service_id UUID NOT NULL REFERENCES project_app_services(id) ON DELETE CASCADE,
    path TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 512),
    content BYTEA NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    PRIMARY KEY (app_service_id, path)
);

CREATE TABLE html_page_events (
    id UUID PRIMARY KEY,
    app_service_id UUID NOT NULL REFERENCES project_app_services(id) ON DELETE CASCADE,
    occurred_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    event_type TEXT NOT NULL CHECK (char_length(event_type) BETWEEN 1 AND 40),
    path TEXT NOT NULL CHECK (char_length(path) BETWEEN 1 AND 2048),
    referrer TEXT,
    language TEXT,
    timezone TEXT,
    screen_width INTEGER,
    screen_height INTEGER,
    viewport_width INTEGER,
    viewport_height INTEGER,
    user_agent TEXT,
    session_id TEXT,
    duration_ms INTEGER,
    extra JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE INDEX html_page_events_service_occurred_idx
    ON html_page_events (app_service_id, occurred_at DESC);
