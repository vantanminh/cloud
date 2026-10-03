-- Account-level Knotree Registry connection: one consent per Cloud user grants
-- pull access to that user's whole Registry namespace. Project connections
-- created from it reference the account and always use its live credential.
CREATE TABLE knotree_registry_accounts (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    registry_username TEXT NOT NULL CHECK (char_length(registry_username) BETWEEN 1 AND 128),
    credential_ciphertext TEXT NOT NULL,
    delegated_credential_id UUID NOT NULL,
    credential_expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX knotree_registry_accounts_active_user
    ON knotree_registry_accounts (user_id) WHERE revoked_at IS NULL;

CREATE TABLE registry_account_consent_attempts (
    state_hash BYTEA PRIMARY KEY,
    session_hash BYTEA NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    verifier_ciphertext TEXT NOT NULL,
    return_to TEXT,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX registry_account_consent_attempts_expiry
    ON registry_account_consent_attempts (expires_at);

ALTER TABLE knotree_registry_connections
    ADD COLUMN account_id UUID REFERENCES knotree_registry_accounts(id) ON DELETE CASCADE;

CREATE UNIQUE INDEX knotree_registry_connections_account_repository
    ON knotree_registry_connections (project_id, account_id, repository)
    WHERE account_id IS NOT NULL AND revoked_at IS NULL;
