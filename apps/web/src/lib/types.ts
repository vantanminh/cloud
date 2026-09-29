export type User = {
  id: string
  fullName: string
  email: string
  emailVerified: boolean
}

export type Workspace = {
  id: string
  name: string
}

export type Project = {
  id: string
  name: string
  slug: string
}

export type PostgresResourceStatus = "provisioning" | "ready" | "error"

export type PostgresResource = {
  id: string
  name: string
  resourceType: "postgres"
  status: PostgresResourceStatus
  databaseName: string
  username: string
  host: string
  port: number
  connectionString: string | null
  clusterProvider: "docker" | "kubernetes" | "legacy_shared"
  clusterName?: string
  errorMessage?: string
}

export type AppServiceStatus = "provisioning" | "ready" | "error"

export type AppServiceDeployment = {
  id: string
  status: AppServiceStatus
  currentStep: string
  logs: string[]
  errorMessage?: string
}

export type AppServiceLogs = {
  appServiceId: string
  containerName: string | null
  status: AppServiceStatus
  running: boolean
  lines: string[]
  message: string | null
}

export type AppServiceDatabaseConnection = {
  resourceId: string
  name: string
  databaseName: string
  username: string
  networkName: string
  host: string
  port: number
  environmentVariables: string[]
}

export type AppService = {
  id: string
  name: string
  resourceType: "app"
  status: AppServiceStatus
  image: string
  imageSource: "public" | "github" | "html" | "html_github" | "knotree_registry"
  registryConnectionId?: string | null
  appPort: number
  htmlRepo?: string | null
  htmlBranch?: string | null
  htmlSha?: string | null
  host: string | null
  port: number | null
  serviceUrl: string | null
  publicDomain?: string | null
  publicAccessEnabled?: boolean
  rateLimitRpm?: number
  containerName: string | null
  errorMessage?: string
  autoDeployEnabled?: boolean
  deployedImageDigest?: string | null
  autoDeployCheckedAt?: string | null
  autoDeployError?: string | null
  databaseConnection?: AppServiceDatabaseConnection
  deployment?: AppServiceDeployment
}

export type RedisResourceStatus = "provisioning" | "ready" | "error"

export type RedisResource = {
  id: string
  name: string
  resourceType: "redis"
  status: RedisResourceStatus
  host: string
  port: number
  connectionString: string | null
  clusterProvider: "docker" | "kubernetes" | "legacy_shared"
  clusterName?: string
  errorMessage?: string
  networkAlias: string
  cpuLimit: string
  memoryLimit: string
  storageLimit: string
}

export type GithubConnectionStatus = {
  connected: boolean
  login: string | null
}

export type KnotreeRegistryConnection = {
  id: string
  registryHost: string
  username: string
  repository: string
  verifiedAt: string
}

export type KnotreeRegistryConnectionList = {
  connections: KnotreeRegistryConnection[]
  autoDeployReady: boolean
  consentReady?: boolean
}

export type DatabaseTable = {
  schemaName: string
  tableName: string
  estimatedRows: number
  sizeBytes: number
}

export type DatabaseColumn = {
  name: string
  dataType: string
  nullable: boolean
}

export type DatabaseTableData = {
  schemaName: string
  tableName: string
  columns: DatabaseColumn[]
  rows: Array<Record<string, unknown>>
  limit: number
  offset: number
  rowCount: number
}

export type DatabaseStats = {
  databaseName: string
  sizeBytes: number
  connections: number
  maxConnections: number
  tableCount: number
  estimatedRows: number
}

export type ResourceMetricPoint = {
  timestamp: number
  cpuPercent: number | null
  memoryUsedBytes: number | null
  memoryLimitBytes: number | null
  volumeUsedBytes: number | null
  volumeCapacityBytes: number | null
  networkReceiveBytes: number | null
  networkTransmitBytes: number | null
  diskReadBytes: number | null
  diskWriteBytes: number | null
  publicNetworkReceiveBytes?: number | null
  publicNetworkTransmitBytes?: number | null
  requests?: number | null
  responseTimeMs?: number | null
  requestErrorRate?: number | null
}

export type DatabaseMetricPoint = ResourceMetricPoint
export type AppServiceMetricPoint = ResourceMetricPoint

export type DatabaseMetricsRange = "1h" | "6h" | "24h" | "7d" | "30d"

export type ResourceMetrics = {
  provider: string
  systemMetricsAvailable: boolean
  systemMetricsMessage: string | null
  sampleIntervalSeconds: number
  retentionSeconds: number
  range: DatabaseMetricsRange
  fromTimestamp: number
  toTimestamp: number
  resolutionSeconds: number
  points: ResourceMetricPoint[]
}

export type DatabaseMetrics = ResourceMetrics
export type AppServiceMetrics = ResourceMetrics

export type DatabaseConfig = {
  name: string
  setting: string
  unit: string | null
  description: string
}

export type DatabaseQueryResult = {
  columns: string[]
  rows: Array<Record<string, unknown>>
  rowCount: number
  affectedRows: number
  durationMs: number
  truncated: boolean
}

export type HtmlAnalyticsSummary = {
  pageviews: number
  sessions: number
  avgDurationMs: number
  topPaths: Array<{ name: string; count: number }>
  topReferrers: Array<{ name: string; count: number }>
  browsers: Array<{ name: string; count: number }>
  eventTypes: Array<{ name: string; count: number }>
  recent: Array<{
    occurredAt: string
    eventType: string
    path: string
    referrer: string | null
    sessionId: string | null
  }>
}

export function isHtmlPage(service: Pick<AppService, "imageSource">) {
  return service.imageSource === "html" || service.imageSource === "html_github"
}

export type AuthResponse = {
  user: User
  workspace: Workspace | null
}

export type ApiErrorPayload = {
  error?: {
    code?: string
    message?: string
    fields?: Record<string, string>
  }
}
