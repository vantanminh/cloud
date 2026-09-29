CREATE TABLE registry_consent_attempts (
    state_hash BYTEA PRIMARY KEY,
    session_hash BYTEA NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    workspace_id TEXT NOT NULL,
    project_slug TEXT NOT NULL,
    repository TEXT NOT NULL,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    verifier_ciphertext TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX registry_consent_attempts_expiry ON registry_consent_attempts(expires_at);
ALTER TABLE knotree_registry_connections
    ADD COLUMN delegated_credential_id UUID,
    ADD COLUMN credential_expires_at TIMESTAMPTZ;
