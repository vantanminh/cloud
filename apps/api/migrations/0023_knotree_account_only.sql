-- Knotree Accounts is the only way into Cloud, and Cloud reaches Registry for
-- the signed-in Knotree account through Registry's internal API.

-- Users are identified by their Knotree account (`sub`) only. Emails can change
-- and move between Knotree accounts, so they are no longer unique here.
DROP INDEX IF EXISTS users_email_lower_key;
CREATE INDEX IF NOT EXISTS users_email_lower_idx ON users (lower(email));
ALTER TABLE users ADD COLUMN IF NOT EXISTS username TEXT;

-- Password sign-in is gone; end any session of an account without a Knotree
-- identity. Their data is kept.
UPDATE sessions SET revoked_at = now()
WHERE revoked_at IS NULL
  AND user_id NOT IN (SELECT user_id FROM sso_identities);

-- Each Registry connection records the Knotree account that owns the
-- repository, so auto-deploys and credential renewal act for that account only.
ALTER TABLE knotree_registry_connections
    ADD COLUMN IF NOT EXISTS owner_issuer TEXT,
    ADD COLUMN IF NOT EXISTS owner_subject TEXT;

UPDATE knotree_registry_connections AS connection
SET owner_issuer = account.issuer, owner_subject = account.subject
FROM knotree_registry_accounts AS account
WHERE connection.account_id = account.id
  AND connection.owner_subject IS NULL;

UPDATE knotree_registry_connections AS connection
SET owner_issuer = identity.issuer, owner_subject = identity.subject
FROM sso_identities AS identity
WHERE identity.user_id = connection.user_id
  AND connection.owner_subject IS NULL;

CREATE INDEX IF NOT EXISTS knotree_registry_connections_renewal_idx
    ON knotree_registry_connections (credential_expires_at)
    WHERE revoked_at IS NULL AND owner_subject IS NOT NULL;

-- In-flight consent attempts belong to the retired consent flow.
DELETE FROM registry_consent_attempts;
DELETE FROM registry_account_consent_attempts;
