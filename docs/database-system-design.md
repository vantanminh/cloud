# Database system design

## Scope

Knotree has two PostgreSQL planes:

```text
control plane (shared)                 project plane (dedicated)
users, sessions, workspaces,           one PostgreSQL server/instance,
projects, resource metadata  ───────▶ database, role, and storage per project
```

The control plane never creates project tables in its own database. It stores
only the generated project database name, role name, encrypted password, and
provider/endpoint metadata. The browser receives connection metadata only
after authorization; the password is never stored in frontend state except as
part of the copy action's returned connection string.

Docker App services are a separate deployment plane. Each project can own up to
six App services, represented by image and runtime metadata in the control plane
and executed in separate Docker containers. Each service keeps its own
image/container lifecycle and never shares the PostgreSQL volume or API
connection pool, but Docker resources join the same project-scoped private
network when the Docker provider is active. Both planes apply a hard allocation
of at most 1 vCPU, 1 GiB RAM, and 10 GiB of writable storage; storage reaching
the ceiling stops the resource so writes cannot continue consuming the host.

## Isolation boundary

Creating a PostgreSQL resource is idempotent per project. The first request
creates a generated database credential record, then provisions a dedicated
instance outside the control-plane transaction. A deterministic provider name
makes retries safe:

- Docker development: one `postgres:16-alpine` container and one named volume
  per project, bound to a random localhost port and attached to the project's
  deterministic private network. The container has the shared CPU, memory,
  swap, and writable-layer limits; a one-second storage guard also stops the
  container when its mounted data volume reaches 10 GiB.
- Kubernetes production: one StatefulSet with one pod, one PVC, one Secret, and
  one Service in the configured namespace. The API service account has only
  namespaced RBAC for those resources.

The one-replica StatefulSet is a dedicated server/storage boundary. It is not
HA; a future provider can add replication/failover without changing the
project resource contract.

## Private service networking

Docker resources in a project share a bridge network named
`knotree-net-<project-id>`. The PostgreSQL container has the stable DNS alias
`postgres`, and each App service has a service-scoped container name; a
different project gets a different network, so project services cannot resolve
one another by default.
The host-published ports remain available for the API and local development,
but application-to-database traffic uses the private network and never needs
the random host port.

An App service starts without a database connection. When the user assigns a
ready PostgreSQL resource to that service from its Settings menu, the next
deployment injects these container-only variables:

`DATABASE_URL`, `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, and `PGPASSWORD`.

`PGHOST` is `postgres` and `PGPORT` is `5432`. The API stores the database
resource UUID on the App service, returns only non-secret connection metadata
and variable names, and never returns the password in the App service response.
Creating a database never changes an existing App service until the user
explicitly selects it. Kubernetes App service provisioning remains disabled
until its namespace-scoped equivalent is implemented.

## Management API

All routes require a session and project membership. Mutations also require
the existing double-submit CSRF token.

| Method | Route suffix | Operation |
| --- | --- | --- |
| GET | `database/tables?search=` | list user tables, estimated rows, and sizes |
| POST | `database/tables` | create a table with validated identifiers and allowlisted types |
| GET | `database/table-data?schema=&table=&limit=&offset=` | read paginated rows and column metadata |
| GET | `database/stats` | read database size, connections, table count, and row estimate |
| GET | `database/metrics?range=1h\|6h\|24h\|7d\|30d` | sample live CPU, memory, volume, network RX/TX, and disk read/write metrics plus retained history |
| GET | `database/config` | read a safe allowlist from `pg_settings` |
| POST | `database/query` | execute one parsed SQL statement |

App deployment and GitHub package connection endpoints are kept separate from
the PostgreSQL resource contract:

| Method | Route suffix | Operation |
| --- | --- | --- |
| GET | `app-services` | read up to six App service records for the project |
| POST | `app-services` | validate an image and deploy a new Docker App service |
| PATCH | `app-services/{app_service_id}` | redeploy one service with a new container port |
| PATCH | `app-services/{app_service_id}/database` | assign or remove the service's PostgreSQL connection |
| GET | `app-services/{app_service_id}/metrics?range=1h\|6h\|24h\|7d\|30d` | sample live App service CPU, memory, volume, network RX/TX, and disk read/write metrics plus retained history |
| GET | `auth/github/status` | report the signed-in user's package connection |
| GET | `auth/github/start` | create OAuth state and return the GitHub authorization URL |
| GET | `auth/github/callback` | exchange the OAuth code and store an encrypted package token |

The API resolves the resource against the authorized project before reusing a
process-local connection pool for that dedicated endpoint. Pools keep zero
minimum connections, expire idle connections after five minutes, and cap each
resource at four connections. Every target connection receives the configured
PostgreSQL `statement_timeout`. Docker development normalizes `localhost` to
IPv4 loopback because Docker Desktop publishes the project port there. Table
reads are identifier quoted after validation and limited to
`DATABASE_QUERY_MAX_ROWS`. SQL console results are capped at the same limit
and report truncation.

The SQL console intentionally rejects cross-cluster administration and server
escape operations (`CREATE/DROP DATABASE`, role administration, `COPY`,
`SET/RESET`, `GRANT/REVOKE`, function/procedure execution, and similar
operations). Project-local DDL/DML remains available through the project
credential.

## Frontend behavior

The resource workspace reads the tables from the selected project database,
opens a table to show live rows, creates tables through the table builder,
runs SQL through the query endpoint, loads stats/config on demand, and polls
metrics every five seconds while the Metrics tab is live. In Docker
development, the metrics collector reads the selected resource's Docker
runtime counters; PostgreSQL reads its mounted data path and App services read
their writable layer. A background sampler records ready databases in
`database_metric_samples` and ready App services in
`app_service_metric_samples` every five seconds, prunes both histories older
than 30 days, and the API returns a bounded, bucketed series for the requested
`1h`, `6h`, `24h`, `7d`, or `30d` range. A separate one-second guard enforces the
10 GiB storage ceiling for Docker resources. Hovering a chart selects the
nearest real sample and shows its timestamp and metric values; providers
without a runtime adapter expose unavailable fields explicitly. Table search
is debounced, identical in-flight reads are deduplicated, and stale table
responses cannot overwrite newer searches. The starter query is `SELECT 1`
until a real table is available, then it is filled with a safely quoted
schema/table name. Loading, authorization, unavailable-resource, and SQL
errors are displayed as safe messages; the UI does not fabricate tables,
metrics, or configuration values.

The App service API supports public registry images without credentials. Private
image support is currently limited to `ghcr.io`: the user connects GitHub with
OAuth, the API encrypts the returned package token with
`DATABASE_CREDENTIALS_ENCRYPTION_KEY`, performs a short-lived Docker registry
login for the pull, then logs out. Tokens are never sent to the browser or
included in API responses. Docker development publishes each container on a
random loopback port and returns the configured public host plus that port as
`serviceUrl`. When a user has assigned a ready Docker PostgreSQL resource, the
same response includes `databaseConnection` with the internal network name,
`postgres:5432`, the database identity, and the names of the assigned
variables. This Docker implementation is enabled by default only in local
development; the production Kubernetes chart keeps App service provisioning
disabled until a Docker runtime integration and public routing layer are
configured for the API deployment.

The App service Metrics tab uses the same five-second live polling, bounded
history, and hover detail as the database Metrics tab. Its volume series tracks
the container writable layer, while the Docker storage guard also handles
mounted data paths. The resource Settings tab shows the enforced 1 vCPU, 1 GB
RAM, and 10 GB storage allocation for both resource types.

## Migration note

Rows created before the dedicated provider were migrated as
`cluster_provider=legacy_shared`. They are not silently reinterpreted as
dedicated resources because doing so could point a project at another
project's server or discard existing data. A data-preserving migration tool
must dump the legacy database, restore it into a newly provisioned project
instance, verify the returned credentials, and then update the resource
metadata as one explicit operational action.
