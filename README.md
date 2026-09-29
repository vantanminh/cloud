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
  # Point *.knotree.org at the API ingress to use generated public URLs.
  # Set the domain to "" to keep the localhost URL fallback.
  $env:APP_SERVICE_PUBLIC_DOMAIN = "knotree.org"
  $env:APP_SERVICE_PUBLIC_SCHEME = "https"
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

For production public App services, set `APP_SERVICE_PUBLIC_DOMAIN=knotree.org`
and point the wildcard DNS record `*.knotree.org` to the API ingress. Knotree
assigns each service a stable random subdomain such as
`app-0123456789abcdef.knotree.org` and routes that hostname to the matching
Docker container. The ingress TLS certificate must cover `*.knotree.org`.
Requests to a wildcard host that is not assigned to an app service show a Knotree
404 error page.

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
- `GET /workspaces/:workspaceId`
- `GET /workspaces/:workspaceId/projects`
- `POST /workspaces/:workspaceId/projects` (optional custom project slug)
- `GET /workspaces/:workspaceId/projects/:projectSlug/resources`
- `POST /workspaces/:workspaceId/projects/:projectSlug/resources`
- `GET /workspaces/:workspaceId/projects/:projectSlug/app-services`
- `POST /workspaces/:workspaceId/projects/:projectSlug/app-services`
- `PATCH /workspaces/:workspaceId/projects/:projectSlug/app-services/:appServiceId/auto-deploy`
- `ANY https://<publicSubdomain>.<APP_SERVICE_PUBLIC_DOMAIN>/*` (host-based public App service proxy)
- `GET /workspaces/:workspaceId/projects/:projectSlug/app-services/:appServiceId/logs` (recent Docker runtime logs)
- `GET /workspaces/:workspaceId/projects/:projectSlug/app-services/:appServiceId/metrics?range=1h|6h|24h|7d|30d`
- `GET /workspaces/:workspaceId/projects/:projectSlug/app-services/deployments/:deploymentId/events` (SSE deployment progress/log stream)
- `GET /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/tables`
- `POST /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/tables`
- `GET /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/table-data`
- `GET /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/stats`
- `GET /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/metrics?range=1h|6h|24h|7d|30d`
- `GET /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/config`
- `POST /workspaces/:workspaceId/projects/:projectSlug/resources/:resourceId/database/query`

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
the Docker pull and then logs out of the registry. Docker keeps the container
on a random loopback port, while `serviceUrl` points to the API public gateway
so public requests can be measured before being forwarded to the container.
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
network, and disk counters for App services as for PostgreSQL. App service
metrics also include public inbound/outbound payload bytes, request count,
average response time, and the percentage of public requests returning 4xx or
5xx responses.

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
`deploy/helm/knotree-api` expects an existing secret containing `DATABASE_URL`.
Its SQLx pre-install/pre-upgrade migration hook is disabled by default; enable it
with `--set migrations.enabled=true` for schema-changing releases. The
production `deploy/k3s-pull.sh` flow enables that hook. The chart exposes
`cloudapi.knotree.com` plus the configured public App service wildcard through
Traefik with a cert-manager `Certificate`.

```powershell
kubectl create secret generic knotree-api-secrets `
  --from-literal=DATABASE_URL='postgres://user:password@postgres.example/knotree_cloud' `
  --from-literal=DATABASE_CREDENTIALS_ENCRYPTION_KEY='replace-with-a-stable-32-byte-base64url-key' `
  --from-literal=GITHUB_CLIENT_ID='your-github-oauth-client-id' `
  --from-literal=GITHUB_CLIENT_SECRET='your-github-oauth-client-secret' `
  --namespace knotree
helm upgrade --install knotree-api deploy/helm/knotree-api `
  --namespace knotree --create-namespace `
  --set migrations.enabled=true `
  --set image.repository=ghcr.io/knotree/knotree-api `
  --set image.tag=0.1.0
```

Use `deploy/helm/knotree-api/values-dev.yaml` with local or non-TLS clusters;
it keeps email verification disabled and provisions project databases through
the Kubernetes provider. For host-local development, use the `.env` Docker
settings above instead of running the API inside Kubernetes.

## Knotree Accounts sign-in

Configure `SSO_ENABLED=true`, `SSO_ISSUER` (the trusted Accounts HTTPS
origin), `SSO_CLIENT_ID=knotree-cloud`, `SSO_REDIRECT_URI` and
`SSO_FRONTEND_URL` after Accounts is deployed. Register the exact callback
`https://cloud.knotree.com/api/v1/auth/sso/callback` on the Accounts public
OAuth client. The Helm chart exposes corresponding `env.sso*` settings.
Until enabled, the existing password sign-in remains available.

The login screen shows **Continue with Knotree** when the API reports SSO
configured. The API uses authorization code and S256 PKCE, a short-lived
browser-bound state, and the trusted Accounts userinfo endpoint to create a
Cloud session. Access tokens are used only during sign-in. Cloud maps users
by issuer and subject, requires verified email, and never silently attaches
an existing account based on email. A collision requires an explicit account
linking flow (not yet implemented). New SSO users cannot log in using a Cloud
password. This integration is not enabled in production until Accounts and
its exact OAuth callback registration are ready.
