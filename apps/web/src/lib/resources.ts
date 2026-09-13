import { apiRequest, apiUrl } from "@/lib/api"
import type {
  DatabaseConfig,
  DatabaseMetrics,
  DatabaseMetricsRange,
  DatabaseQueryResult,
  DatabaseStats,
  DatabaseTable,
  DatabaseTableData,
  AppService,
  AppServiceLogs,
  GithubConnectionStatus,
  PostgresResource,
} from "@/lib/types"

export type CreatePostgresResourceInput = {
  name: string
}

export type CreateAppServiceInput = {
  name: string
  image: string
  imageSource: "public" | "github"
  appPort: number
}

export type UpdateAppServiceInput = {
  appPort: number
}

const inFlightDatabaseRequests = new Map<string, Promise<unknown>>()

function deduplicateDatabaseRequest<T>(key: string, request: () => Promise<T>) {
  const existing = inFlightDatabaseRequests.get(key)
  if (existing) {
    return existing as Promise<T>
  }

  const pending = request()
  inFlightDatabaseRequests.set(key, pending)
  void pending.then(
    () => {
      if (inFlightDatabaseRequests.get(key) === pending) {
        inFlightDatabaseRequests.delete(key)
      }
    },
    () => {
      if (inFlightDatabaseRequests.get(key) === pending) {
        inFlightDatabaseRequests.delete(key)
      }
    }
  )
  return pending
}

function resourcesPath(workspaceSlug: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}/resources`
}

function appServicesPath(workspaceSlug: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}/app-services`
}

export function appServiceDeploymentEventsUrl(
  workspaceSlug: string,
  projectSlug: string,
  deploymentId: string
) {
  return apiUrl(
    `${appServicesPath(workspaceSlug, projectSlug)}/deployments/${encodeURIComponent(deploymentId)}/events`
  )
}

export function getAppServiceLogs(
  workspaceSlug: string,
  projectSlug: string,
  appServiceId: string
) {
  return apiRequest<AppServiceLogs>(
    `${appServicesPath(workspaceSlug, projectSlug)}/${encodeURIComponent(appServiceId)}/logs`
  )
}

function databasePath(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  suffix: string
) {
  return `${resourcesPath(workspaceSlug, projectSlug)}/${encodeURIComponent(resourceId)}/database/${suffix}`
}

export function listPostgresResources(
  workspaceSlug: string,
  projectSlug: string
) {
  return apiRequest<PostgresResource[]>(
    resourcesPath(workspaceSlug, projectSlug)
  )
}

export function createPostgresResource(
  workspaceSlug: string,
  projectSlug: string,
  input: CreatePostgresResourceInput
) {
  return apiRequest<PostgresResource>(
    resourcesPath(workspaceSlug, projectSlug),
    {
      method: "POST",
      body: { resourceType: "postgres", name: input.name },
    }
  )
}

export function listAppServices(workspaceSlug: string, projectSlug: string) {
  return apiRequest<AppService[]>(appServicesPath(workspaceSlug, projectSlug))
}

export function createAppService(
  workspaceSlug: string,
  projectSlug: string,
  input: CreateAppServiceInput
) {
  return apiRequest<AppService>(appServicesPath(workspaceSlug, projectSlug), {
    method: "POST",
    body: input,
  })
}

export function updateAppService(
  workspaceSlug: string,
  projectSlug: string,
  input: UpdateAppServiceInput
) {
  return apiRequest<AppService>(appServicesPath(workspaceSlug, projectSlug), {
    method: "PATCH",
    body: input,
  })
}

export function getGithubConnectionStatus() {
  return apiRequest<GithubConnectionStatus>("/auth/github/status")
}

export function getGithubAuthorizationUrl(returnTo?: string) {
  const query = returnTo ? `?returnTo=${encodeURIComponent(returnTo)}` : ""
  return apiRequest<{ authorizationUrl: string }>(`/auth/github/start${query}`)
}

export function disconnectGithub() {
  return apiRequest<void>("/auth/github/disconnect", { method: "POST" })
}

export function listDatabaseTables(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  search?: string
) {
  const query = search ? `?search=${encodeURIComponent(search)}` : ""
  const path = `${databasePath(workspaceSlug, projectSlug, resourceId, "tables")}${query}`
  return deduplicateDatabaseRequest(path, () =>
    apiRequest<DatabaseTable[]>(path)
  )
}

export function getDatabaseTableData(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  tableName: string,
  schemaName = "public",
  limit = 50,
  offset = 0
) {
  const query = new URLSearchParams({
    schema: schemaName,
    table: tableName,
    limit: String(limit),
    offset: String(offset),
  })
  const path = `${databasePath(workspaceSlug, projectSlug, resourceId, "table-data")}?${query.toString()}`
  return deduplicateDatabaseRequest(path, () =>
    apiRequest<DatabaseTableData>(path)
  )
}

export type CreateDatabaseTableInput = {
  name: string
  schema?: string
  columns: Array<{
    name: string
    dataType: string
    nullable?: boolean
    primaryKey?: boolean
  }>
}

export function createDatabaseTable(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  input: CreateDatabaseTableInput
) {
  return apiRequest<{ schemaName: string; tableName: string }>(
    databasePath(workspaceSlug, projectSlug, resourceId, "tables"),
    { method: "POST", body: input }
  )
}

export function getDatabaseStats(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string
) {
  const path = databasePath(workspaceSlug, projectSlug, resourceId, "stats")
  return deduplicateDatabaseRequest(path, () => apiRequest<DatabaseStats>(path))
}

export function getDatabaseMetrics(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  range: DatabaseMetricsRange = "24h"
) {
  const query = new URLSearchParams({ range })
  const path = `${databasePath(workspaceSlug, projectSlug, resourceId, "metrics")}?${query.toString()}`
  return apiRequest<DatabaseMetrics>(path)
}

export function getDatabaseConfig(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string
) {
  const path = databasePath(workspaceSlug, projectSlug, resourceId, "config")
  return deduplicateDatabaseRequest(path, () =>
    apiRequest<DatabaseConfig[]>(path)
  )
}

export function executeDatabaseQuery(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  sql: string
) {
  return apiRequest<DatabaseQueryResult>(
    databasePath(workspaceSlug, projectSlug, resourceId, "query"),
    { method: "POST", body: { sql } }
  )
}
