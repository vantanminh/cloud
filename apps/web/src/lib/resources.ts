import { apiRequest } from "@/lib/api"
import type {
  DatabaseConfig,
  DatabaseQueryResult,
  DatabaseStats,
  DatabaseTable,
  DatabaseTableData,
  PostgresResource,
} from "@/lib/types"

export type CreatePostgresResourceInput = {
  name: string
}

function resourcesPath(workspaceSlug: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}/resources`
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

export function listDatabaseTables(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string,
  search?: string
) {
  const query = search ? `?search=${encodeURIComponent(search)}` : ""
  return apiRequest<DatabaseTable[]>(
    `${databasePath(workspaceSlug, projectSlug, resourceId, "tables")}${query}`
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
  return apiRequest<DatabaseTableData>(
    `${databasePath(workspaceSlug, projectSlug, resourceId, "table-data")}?${query.toString()}`
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
  return apiRequest<DatabaseStats>(
    databasePath(workspaceSlug, projectSlug, resourceId, "stats")
  )
}

export function getDatabaseConfig(
  workspaceSlug: string,
  projectSlug: string,
  resourceId: string
) {
  return apiRequest<DatabaseConfig[]>(
    databasePath(workspaceSlug, projectSlug, resourceId, "config")
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
