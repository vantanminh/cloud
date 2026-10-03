import { apiBinaryRequest, apiRequest, apiUrl } from "@/lib/api"
import type {
  DatabaseConfig,
  DatabaseMetrics,
  DatabaseMetricsRange,
  DatabaseQueryResult,
  DatabaseStats,
  DatabaseTable,
  DatabaseTableData,
  AppService,
  AppServiceMetrics,
  AppServiceLogs,
  GithubConnectionStatus,
  KnotreeRegistryConnection,
  KnotreeRegistryConnectionList,
  HtmlAnalyticsSummary,
  ImageApiKey,
  ImageCompressionMode,
  ImageKeyAccess,
  ImageObjectList,
  ImageStore,
  RegistryDeployHistory,
  PostgresResource,
  RedisResource,
  SignedImageUrl,
} from "@/lib/types"

export type CreatePostgresResourceInput = {
  name: string
}

export type CreateAppServiceInput = {
  name: string
  image?: string
  imageSource: "public" | "github" | "html" | "html_github" | "knotree_registry"
  appPort?: number
  autoDeploy?: boolean
  pageSlug?: string
  indexHtml?: string
  githubRepo?: string
  githubBranch?: string
  registryConnectionId?: string
}

export type CreateKnotreeRegistryConnectionInput = {
  username: string
  token: string
  repository: string
}

export type UpdateAppServiceInput = {
  appPort: number
}

export type UpdateAppServiceAutoDeployInput = {
  enabled: boolean
}

export type UpdateAppServiceDatabaseInput = {
  databaseResourceId: string | null
}

export type CreateRedisResourceInput = {
  name: string
}

export type UpdateAppServicePublicAccessInput = {
  enabled: boolean
  rateLimitRpm?: number
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

function resourcesPath(workspaceId: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceId)}/projects/${encodeURIComponent(projectSlug)}/resources`
}

function appServicesPath(workspaceId: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceId)}/projects/${encodeURIComponent(projectSlug)}/app-services`
}

function knotreeRegistryConnectionsPath(
  workspaceId: string,
  projectSlug: string
) {
  return `/workspaces/${encodeURIComponent(workspaceId)}/projects/${encodeURIComponent(projectSlug)}/registry-connections`
}

export function listKnotreeRegistryConnections(
  workspaceId: string,
  projectSlug: string
) {
  return apiRequest<KnotreeRegistryConnectionList>(
    knotreeRegistryConnectionsPath(workspaceId, projectSlug)
  )
}

export function createKnotreeRegistryConnection(
  workspaceId: string,
  projectSlug: string,
  input: CreateKnotreeRegistryConnectionInput
) {
  return apiRequest<KnotreeRegistryConnection>(
    knotreeRegistryConnectionsPath(workspaceId, projectSlug),
    { method: "POST", body: input }
  )
}

export function updateKnotreeRegistryConnection(
  workspaceId: string,
  projectSlug: string,
  connectionId: string,
  token: string
) {
  return apiRequest<KnotreeRegistryConnection>(
    `${knotreeRegistryConnectionsPath(workspaceId, projectSlug)}/${encodeURIComponent(connectionId)}`,
    { method: "PATCH", body: { token } }
  )
}

export function deleteKnotreeRegistryConnection(
  workspaceId: string,
  projectSlug: string,
  connectionId: string
) {
  return apiRequest<void>(
    `${knotreeRegistryConnectionsPath(workspaceId, projectSlug)}/${encodeURIComponent(connectionId)}`,
    { method: "DELETE" }
  )
}

export function attachAppServiceRegistryConnection(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  connectionId: string
) {
  return apiRequest<AppService>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/registry-connection`,
    { method: "PATCH", body: { connectionId } }
  )
}

export function listRegistryDeploys(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string
) {
  return apiRequest<RegistryDeployHistory>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/registry-deploys`
  )
}

export function appServiceDeploymentEventsUrl(
  workspaceId: string,
  projectSlug: string,
  deploymentId: string
) {
  return apiUrl(
    `${appServicesPath(workspaceId, projectSlug)}/deployments/${encodeURIComponent(deploymentId)}/events`
  )
}

export function getAppServiceLogs(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string
) {
  return apiRequest<AppServiceLogs>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/logs`
  )
}

export function getAppServiceMetrics(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  range: DatabaseMetricsRange = "24h"
) {
  const query = new URLSearchParams({ range })
  const path = `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/metrics?${query.toString()}`
  return apiRequest<AppServiceMetrics>(path)
}

function databasePath(
  workspaceId: string,
  projectSlug: string,
  resourceId: string,
  suffix: string
) {
  return `${resourcesPath(workspaceId, projectSlug)}/${encodeURIComponent(resourceId)}/database/${suffix}`
}

export function listPostgresResources(
  workspaceId: string,
  projectSlug: string
) {
  return apiRequest<PostgresResource[]>(resourcesPath(workspaceId, projectSlug))
}

export function createPostgresResource(
  workspaceId: string,
  projectSlug: string,
  input: CreatePostgresResourceInput
) {
  return apiRequest<PostgresResource>(resourcesPath(workspaceId, projectSlug), {
    method: "POST",
    body: { resourceType: "postgres", name: input.name },
  })
}

export function retryPostgresResource(
  workspaceId: string,
  projectSlug: string,
  resourceId: string
) {
  return apiRequest<PostgresResource>(
    `${resourcesPath(workspaceId, projectSlug)}/${encodeURIComponent(resourceId)}/retry`,
    { method: "POST" }
  )
}

function redisPath(workspaceId: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceId)}/projects/${encodeURIComponent(projectSlug)}/redis`
}

export function listRedisResources(workspaceId: string, projectSlug: string) {
  return apiRequest<RedisResource[]>(redisPath(workspaceId, projectSlug))
}

export function createRedisResource(
  workspaceId: string,
  projectSlug: string,
  input: CreateRedisResourceInput
) {
  return apiRequest<RedisResource>(redisPath(workspaceId, projectSlug), {
    method: "POST",
    body: { resourceType: "redis", name: input.name },
  })
}

export function updateAppServicePublicAccess(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  input: UpdateAppServicePublicAccessInput
) {
  return apiRequest<AppService>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/public-access`,
    {
      method: "PATCH",
      body: input,
    }
  )
}

export function listAppServices(workspaceId: string, projectSlug: string) {
  return apiRequest<AppService[]>(appServicesPath(workspaceId, projectSlug))
}

export function createAppService(
  workspaceId: string,
  projectSlug: string,
  input: CreateAppServiceInput
) {
  return apiRequest<AppService>(appServicesPath(workspaceId, projectSlug), {
    method: "POST",
    body: input,
  })
}

export function updateAppService(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  input: UpdateAppServiceInput
) {
  return apiRequest<AppService>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}`,
    {
      method: "PATCH",
      body: input,
    }
  )
}

export function updateAppServiceAutoDeploy(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  input: UpdateAppServiceAutoDeployInput
) {
  return apiRequest<AppService>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/auto-deploy`,
    {
      method: "PATCH",
      body: input,
    }
  )
}

export function getHtmlPageIndex(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string
) {
  return apiRequest<{ indexHtml: string }>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/html`
  )
}

export function updateHtmlPage(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  input: { indexHtml: string }
) {
  return apiRequest<AppService>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/html`,
    { method: "PATCH", body: input }
  )
}

export function getHtmlPageAnalytics(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string
) {
  return apiRequest<HtmlAnalyticsSummary>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/html-analytics`
  )
}

export function updateAppServiceDatabase(
  workspaceId: string,
  projectSlug: string,
  appServiceId: string,
  input: UpdateAppServiceDatabaseInput
) {
  return apiRequest<AppService>(
    `${appServicesPath(workspaceId, projectSlug)}/${encodeURIComponent(appServiceId)}/database`,
    {
      method: "PATCH",
      body: input,
    }
  )
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
  workspaceId: string,
  projectSlug: string,
  resourceId: string,
  search?: string
) {
  const query = search ? `?search=${encodeURIComponent(search)}` : ""
  const path = `${databasePath(workspaceId, projectSlug, resourceId, "tables")}${query}`
  return deduplicateDatabaseRequest(path, () =>
    apiRequest<DatabaseTable[]>(path)
  )
}

export function getDatabaseTableData(
  workspaceId: string,
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
  const path = `${databasePath(workspaceId, projectSlug, resourceId, "table-data")}?${query.toString()}`
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
  workspaceId: string,
  projectSlug: string,
  resourceId: string,
  input: CreateDatabaseTableInput
) {
  return apiRequest<{ schemaName: string; tableName: string }>(
    databasePath(workspaceId, projectSlug, resourceId, "tables"),
    { method: "POST", body: input }
  )
}

export function getDatabaseStats(
  workspaceId: string,
  projectSlug: string,
  resourceId: string
) {
  const path = databasePath(workspaceId, projectSlug, resourceId, "stats")
  return deduplicateDatabaseRequest(path, () => apiRequest<DatabaseStats>(path))
}

export function getDatabaseMetrics(
  workspaceId: string,
  projectSlug: string,
  resourceId: string,
  range: DatabaseMetricsRange = "24h"
) {
  const query = new URLSearchParams({ range })
  const path = `${databasePath(workspaceId, projectSlug, resourceId, "metrics")}?${query.toString()}`
  return apiRequest<DatabaseMetrics>(path)
}

export function getDatabaseConfig(
  workspaceId: string,
  projectSlug: string,
  resourceId: string
) {
  const path = databasePath(workspaceId, projectSlug, resourceId, "config")
  return deduplicateDatabaseRequest(path, () =>
    apiRequest<DatabaseConfig[]>(path)
  )
}

export function executeDatabaseQuery(
  workspaceId: string,
  projectSlug: string,
  resourceId: string,
  sql: string
) {
  return apiRequest<DatabaseQueryResult>(
    databasePath(workspaceId, projectSlug, resourceId, "query"),
    { method: "POST", body: { sql } }
  )
}

export function startKnotreeRegistryConsent(
  workspaceId: string,
  projectSlug: string,
  repository: string
) {
  return apiRequest<{ authorizationUrl: string }>(
    `${knotreeRegistryConnectionsPath(workspaceId, projectSlug)}/authorize`,
    { method: "POST", body: { repository } }
  )
}

function imageStorePath(workspaceId: string, projectSlug: string, storeId = "") {
  const base = `/workspaces/${workspaceId}/projects/${projectSlug}/image-stores`
  return storeId ? `${base}/${storeId}` : base
}

export type CreateImageStoreInput = {
  name: string
  compressionMode: ImageCompressionMode
  maxWidth?: number
  maxHeight?: number
  quality?: number
}

export function listImageStores(workspaceId: string, projectSlug: string) {
  return apiRequest<ImageStore[]>(imageStorePath(workspaceId, projectSlug))
}

export function createImageStore(
  workspaceId: string,
  projectSlug: string,
  input: CreateImageStoreInput
) {
  return apiRequest<ImageStore>(imageStorePath(workspaceId, projectSlug), {
    method: "POST",
    body: input,
  })
}

export function updateImageStore(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  input: CreateImageStoreInput
) {
  return apiRequest<ImageStore>(imageStorePath(workspaceId, projectSlug, storeId), {
    method: "PATCH",
    body: input,
  })
}

export function listImageKeys(
  workspaceId: string,
  projectSlug: string,
  storeId: string
) {
  return apiRequest<ImageApiKey[]>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/keys`
  )
}

export function createImageKey(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  input: { name: string; access: ImageKeyAccess }
) {
  return apiRequest<ImageApiKey>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/keys`,
    { method: "POST", body: input }
  )
}

export function revokeImageKey(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  keyId: string
) {
  return apiRequest<ImageApiKey>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/keys/${keyId}/revoke`,
    { method: "POST", body: {} }
  )
}

export function listImageObjects(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  folder?: string
) {
  const query = new URLSearchParams({ recursive: "true" })
  if (folder) {
    query.set("folder", folder)
  }
  return apiRequest<ImageObjectList>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/objects?${query.toString()}`
  )
}

export function uploadImageObject(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  file: File,
  folder: string
) {
  return apiBinaryRequest<ImageObjectList["objects"][number]>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/objects`,
    file,
    {
      "Content-Type": file.type || "application/octet-stream",
      "X-Knotree-Folder": folder,
      "X-Knotree-File-Name": file.name,
    }
  )
}

export function deleteImageObject(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  imageId: string
) {
  return apiRequest<void>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/objects/${imageId}`,
    { method: "DELETE" }
  )
}

export function deleteImageFolder(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  folder: string
) {
  const query = new URLSearchParams({ folder })
  return apiRequest<{ deleted: number }>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/folders?${query.toString()}`,
    { method: "DELETE" }
  )
}

export function signImageObject(
  workspaceId: string,
  projectSlug: string,
  storeId: string,
  imageId: string,
  input: {
    visibility: "public" | "private"
    expiresInSeconds?: number
    width?: number
    height?: number
    quality?: number
    keyId?: string
  }
) {
  return apiRequest<SignedImageUrl>(
    `${imageStorePath(workspaceId, projectSlug, storeId)}/objects/${imageId}/sign`,
    { method: "POST", body: input }
  )
}
