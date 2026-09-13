# Knotree Cloud

Knotree Cloud is a Rust/PostgreSQL control plane with a Vite/React dashboard.
Each new project can provision and manage its own dedicated PostgreSQL instance
and one Docker App service.

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
$env:DATABASE_RESOURCE_HOST = "127.0.0.1"
$env:DATABASE_RESOURCE_PORT = "5432"
$env:DATABASE_CLUSTER_PROVIDER = "docker"
$env:DATABASE_CLUSTER_IMAGE = "postgres:16-alpine"
$env:DATABASE_CLUSTER_DOCKER_BINARY = "docker"
# A random host port is allocated for each project container.
$env:DATABASE_CLUSTER_BIND_ADDRESS = "127.0.0.1"
$env:DATABASE_CLUSTER_STARTUP_TIMEOUT_SECONDS = "90"
  $env:DATABASE_QUERY_TIMEOUT_MS = "10000"
  $env:DATABASE_QUERY_MAX_ROWS = "500"
  $env:APP_SERVICE_PROVISIONING_ENABLED = "true"
  $env:APP_SERVICE_PUBLIC_HOST = "localhost"
  $env:APP_SERVICE_BIND_ADDRESS = "127.0.0.1"
  # Keep this stable so credentials remain readable after an API restart.
  $env:DATABASE_CREDENTIALS_ENCRYPTION_KEY = "AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA"
  # Optional for private ghcr.io images; public images do not need these.
  $env:GITHUB_CLIENT_ID = ""
  $env:GITHUB_CLIENT_SECRET = ""
  $env:GITHUB_OAUTH_REDIRECT_URI = "http://localhost:8080/api/v1/auth/github/callback"
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
- `GET /auth/github/status`
- `GET /auth/github/start?returnTo=/workspace/...`
- `GET /auth/github/callback`
- `POST /auth/github/disconnect`
- `POST /workspaces`
- `GET /workspaces/:slug`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/resources`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/app-services`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/app-services`
- `PATCH /workspaces/:workspaceSlug/projects/:projectSlug/app-services/:appServiceId/auto-deploy`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/app-services/:appServiceId/logs` (recent Docker runtime logs)
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/app-services/deployments/:deploymentId/events` (SSE deployment progress/log stream)
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/tables`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/tables`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/table-data`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/stats`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/metrics?range=1h|6h|24h|7d|30d`
- `GET /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/config`
- `POST /workspaces/:workspaceSlug/projects/:projectSlug/resources/:resourceId/database/query`

Sessions are opaque, server-side records in PostgreSQL. The browser receives an
HttpOnly session cookie plus a short-lived in-memory CSRF token for mutating
requests. Production enables secure `__Host-` cookies and exact-origin CORS.

The Postgres resource endpoint provisions one dedicated PostgreSQL instance per
project. The control-plane database in `DATABASE_URL` stores only resource
metadata and encrypted credentials; it is not used as a project database.

The App service endpoint provisions one Docker container per project from the
submitted image reference. Public images are pulled without credentials. Private
images are currently restricted to `ghcr.io` and require the signed-in user to
connect GitHub from the account-level `/settings/integrations` page (or from the
deploy dialog). Each Knotree user has an independent GitHub connection; the API
stores that user's encrypted OAuth package token only long enough to authenticate
the Docker pull and then logs out of the registry. The response includes the
random published port and `serviceUrl`.
GitHub-sourced services can automatically poll their GHCR tag once per minute;
when the pulled image identity changes, the API queues a normal redeployment.
The Settings panel can enable or disable this watcher and reports the last
checked image identity and any safe registry error. A failed check leaves the
currently running container untouched.
The local Docker implementation is enabled by default in development. The
production Kubernetes deployment keeps it disabled until the API has an
available Docker runtime and a public routing layer for app containers.

Docker App services and their project's PostgreSQL resource automatically join
the same project-scoped private bridge network
(`knotree-net-<project-id>`). The database is reachable from the App service
at `postgres:5432`, without using the random host port. When Postgres is
ready, the App service receives `DATABASE_URL` plus `PGHOST`,
`PGPORT`, `PGDATABASE`, `PGUSER`, and `PGPASSWORD` inside
the container. The API response exposes only the assigned resource metadata
and variable names, never secret values. Creating the resources in either
order is supported; creating Postgres after an App service triggers an
automatic app reconciliation.

Both PostgreSQL and App service containers are hard-capped at 1 vCPU, 1 GB RAM,
and 10 GB writable storage. Docker runtime limits are applied at creation and
reconciled for existing containers; a one-second storage guard stops a
resource at the 10 GB ceiling and marks it with a safe error instead of
allowing host-wide growth. The Metrics tab and the corresponding
`app-services/:id/metrics` endpoint expose the same CPU, memory, volume,
network, and disk counters for App services as for PostgreSQL.

Development uses the local Docker daemon and creates one container plus one
named volume per project (`knotree-pg-<project-id>` and
`knotree-pg-data-<project-id>`). Production uses Kubernetes and creates one
StatefulSet, Secret, Service, and PVC per project. The Kubernetes chart grants
the API service account only namespaced permissions for those resources. A
single StatefulSet replica is an isolation boundary, not a high-availability
cluster; replication/failover is a separate provider capability.

The project database API connects to the dedicated instance and supports table
introspection, paginated table data, table creation, bounded SQL execution,
live statistics, runtime metrics, and selected PostgreSQL settings. SQL requests run with a
per-connection statement timeout and a configurable result-row limit; cluster
administration statements such as role/database creation, `COPY`, `SET`, and
`GRANT` are rejected by the console.

The Metrics endpoints sample the selected project container's Docker runtime
stats in development: CPU, memory, network receive/transmit totals, block
disk read/write totals, and volume usage/capacity. A background sampler
persists ready database and App service samples in separate control-plane
tables for up to 30 days; `?range=1h|6h|24h|7d|30d` returns a bounded bucketed
history for the charts. Kubernetes providers return explicit unavailable
runtime fields until a cluster metrics adapter is configured; the database PVC
is still capped at 10Gi and rejects configuration above that ceiling.

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
  --from-literal=GITHUB_CLIENT_ID='your-github-oauth-client-id' `
  --from-literal=GITHUB_CLIENT_SECRET='your-github-oauth-client-secret' `
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
