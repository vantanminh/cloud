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

Sessions are opaque, server-side records in PostgreSQL. The browser receives an
HttpOnly session cookie plus a short-lived in-memory CSRF token for mutating
requests. Production enables secure `__Host-` cookies and exact-origin CORS.

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
  --namespace knotree
helm upgrade --install knotree-api deploy/helm/knotree-api `
  --namespace knotree --create-namespace `
  --set image.repository=ghcr.io/knotree/knotree-api `
  --set image.tag=0.1.0
```

Use `deploy/helm/knotree-api/values-dev.yaml` with local or non-TLS clusters;
it keeps email verification disabled.
