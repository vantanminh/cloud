# Knotree Cloud

Knotree Cloud is a Rust/PostgreSQL control plane with a Vite/React dashboard.
Each new project can provision and manage its own dedicated PostgreSQL
instance.

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
$env:DATABASE_CLUSTER_PROVIDER = "docker"
$env:DATABASE_CLUSTER_IMAGE = "postgres:16-alpine"
$env:DATABASE_CLUSTER_DOCKER_BINARY = "docker"
# A random host port is allocated for each project container.
$env:DATABASE_CLUSTER_BIND_ADDRESS = "127.0.0.1"
$env:DATABASE_CLUSTER_STARTUP_TIMEOUT_SECONDS = "90"
$env:DATABASE_QUERY_TIMEOUT_MS = "10000"
$env:DATABASE_QUERY_MAX_ROWS = "500"
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
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/tables`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/tables`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/table-data`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/stats`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/config`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/query`

Sessions are opaque, server-side records in PostgreSQL. The browser receives an
HttpOnly session cookie plus a short-lived in-memory CSRF token for mutating
requests. Production enables secure `__Host-` cookies and exact-origin CORS.

The Postgres resource endpoint provisions one dedicated PostgreSQL instance per
project. The control-plane database in `DATABASE_URL` stores only resource
metadata and encrypted credentials; it is not used as a project database.

Development uses the local Docker daemon and creates one container plus one
named volume per project (`knotree-pg-<project-id>` and
`knotree-pg-data-<project-id>`). Production uses Kubernetes and creates one
StatefulSet, Secret, Service, and PVC per project. The Kubernetes chart grants
the API service account only namespaced permissions for those resources. A
single StatefulSet replica is an isolation boundary, not a high-availability
cluster; replication/failover is a separate provider capability.

The project database API connects to the dedicated instance and supports table
introspection, paginated table data, table creation, bounded SQL execution,
live statistics, and selected PostgreSQL settings. SQL requests run with a
per-connection statement timeout and a configurable result-row limit; cluster
administration statements such as role/database creation, `COPY`, `SET`, and
`GRANT` are rejected by the console.

Resource credentials are encrypted at rest with
`DATABASE_CREDENTIALS_ENCRYPTION_KEY`; production must provide a stable,
base64url-encoded 32-byte key. In Kubernetes, `ClusterIP` is the safe default:
the API can manage the database over the cluster network. Set
`env.databaseClusterServiceType=LoadBalancer` plus
`env.databaseResourcePublicHost`/`env.databaseResourcePublicPort` when users
must connect from outside the cluster. Existing resources created by the old
shared-cluster provider are marked `legacy_shared` and are intentionally not
silently moved; migrate their data explicitly before using the dedicated
management endpoints.

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
it keeps email verification disabled and provisions project databases through
the Kubernetes provider. For host-local development, use the `.env` Docker
settings above instead of running the API inside Kubernetes.
