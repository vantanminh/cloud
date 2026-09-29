CREATE TABLE sso_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer, subject),
    UNIQUE (issuer, user_id)
);
CREATE TABLE sso_login_attempts (
    state_hash BYTEA PRIMARY KEY,
    browser_hash BYTEA NOT NULL,
    verifier_ciphertext TEXT NOT NULL,
    issuer TEXT NOT NULL,
    client_id TEXT NOT NULL,
    redirect_uri TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX sso_login_attempts_expiry ON sso_login_attempts(expires_at);
