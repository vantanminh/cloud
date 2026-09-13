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

## Isolation boundary

Creating a PostgreSQL resource is idempotent per project. The first request
creates a generated database credential record, then provisions a dedicated
instance outside the control-plane transaction. A deterministic provider name
makes retries safe:

- Docker development: one `postgres:16-alpine` container and one named volume
  per project, bound to a random localhost port.
- Kubernetes production: one StatefulSet with one pod, one PVC, one Secret, and
  one Service in the configured namespace. The API service account has only
  namespaced RBAC for those resources.

The one-replica StatefulSet is a dedicated server/storage boundary. It is not
HA; a future provider can add replication/failover without changing the
project resource contract.

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
development, the metrics collector reads the selected container's Docker
runtime counters and mounted data-path filesystem usage. A background sampler
records ready dedicated resources every five seconds in the control-plane
`database_metric_samples` table, prunes samples older than 30 days, and the
API returns a bounded, bucketed series for the requested `1h`, `6h`, `24h`,
`7d`, or `30d` range. Hovering a chart selects the nearest real sample and
shows its timestamp and metric values; providers without a runtime adapter
expose unavailable fields explicitly. Table search is debounced, identical
in-flight reads are deduplicated, and stale table responses cannot overwrite
newer searches. The starter query is `SELECT 1` until a real table is
available, then it is filled with a safely quoted schema/table name. Loading,
authorization, unavailable-database, and SQL errors are displayed as safe
messages; the UI does not fabricate tables, metrics, or configuration values.

## Migration note

Rows created before the dedicated provider were migrated as
`cluster_provider=legacy_shared`. They are not silently reinterpreted as
dedicated resources because doing so could point a project at another
project's server or discard existing data. A data-preserving migration tool
must dump the legacy database, restore it into a newly provisioned project
instance, verify the returned credentials, and then update the resource
metadata as one explicit operational action.
