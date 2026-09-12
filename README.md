# Knotree Cloud

MVP authentication and first-workspace onboarding for Knotree Cloud.

## Local development

Requirements: Node.js 24+, pnpm 12+, Rust, and Docker.

```powershell
pnpm install
Copy-Item .env.example .env
docker compose up -d postgres
cargo run --manifest-path apps/api/Cargo.toml -- migrate

# terminal 1
$env:DATABASE_URL = "postgres://postgres:postgres@localhost:5432/knotree_cloud"
$env:APP_ENV = "development"
$env:CORS_ALLOWED_ORIGINS = "http://localhost:5173"
$env:COOKIE_SECURE = "false"
$env:AUTH_REQUIRE_EMAIL_VERIFICATION = "false"
$env:DATABASE_PROVISIONING_ENABLED = "true"
$env:DATABASE_RESOURCE_HOST = "localhost"
$env:DATABASE_RESOURCE_PORT = "5432"
# Keep this stable so credentials remain readable after an API restart.
$env:DATABASE_CREDENTIALS_ENCRYPTION_KEY = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"
cargo run --manifest-path apps/api/Cargo.toml

# terminal 2
pnpm dev:web
```

The web app is available at `http://localhost:5173`. In development, account
registration logs the user in immediately and does not require email
verification. A user without a workspace is sent to `/new/workspace`.

## API

The Rust API uses Axum, SQLx, and PostgreSQL. Routes are under `/api/v1`:

- `GET /auth/csrf`
- `GET /auth/me`
- `POST /auth/register`
- `POST /auth/login`
- `POST /auth/logout`
- `POST /workspaces`
- `GET /workspaces/:slug`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/resources`

Sessions are opaque, server-side records in PostgreSQL. The browser receives an
HttpOnly session cookie plus a short-lived in-memory CSRF token for mutating
requests. Production enables secure `__Host-` cookies and exact-origin CORS.

The Postgres resource endpoint provisions one isolated database and login role
per project on the cluster in `DATABASE_URL`. The API database role must have
`CREATEDB` and `CREATEROLE`. Resource credentials are encrypted at rest with
`DATABASE_CREDENTIALS_ENCRYPTION_KEY`; production must provide a stable,
base64url-encoded 32-byte key. Set `DATABASE_RESOURCE_HOST` to the host that
users can reach; it defaults to the host parsed from `DATABASE_URL`.

## Checks

```powershell
pnpm check
pnpm test:web
pnpm build:web
cargo test --manifest-path apps/api/Cargo.toml
cargo fmt --manifest-path apps/api/Cargo.toml --all -- --check
```

## Deployment

The Vite output is configured for an assets-only Cloudflare Worker at
`cloud.knotree.com`:

```powershell
pnpm --dir apps/web build
pnpm --dir apps/web exec wrangler deploy
```

Set `apps/web/.env.production` (or the CI build environment) to:

```text
VITE_API_BASE_URL=https://cloudapi.knotree.com/api/v1
```

The API image is in `apps/api/Dockerfile`. The Helm chart in
`deploy/helm/knotree-api` expects an existing secret containing `DATABASE_URL`,
runs SQLx migrations as a pre-install/pre-upgrade hook, and exposes
`cloudapi.knotree.com` through Traefik with a cert-manager `Certificate`.

```powershell
kubectl create secret generic knotree-api-secrets `
  --from-literal=DATABASE_URL='postgres://user:password@postgres.example/knotree_cloud' `
  --from-literal=DATABASE_CREDENTIALS_ENCRYPTION_KEY='replace-with-a-stable-32-byte-base64url-key' `
  --namespace knotree
helm upgrade --install knotree-api deploy/helm/knotree-api `
  --namespace knotree --create-namespace `
  --set image.repository=ghcr.io/knotree/knotree-api `
  --set image.tag=0.1.0
```

Use `deploy/helm/knotree-api/values-dev.yaml` with local or non-TLS clusters;
it keeps email verification disabled.
