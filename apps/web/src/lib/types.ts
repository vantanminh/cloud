export type User = {
  id: string
  fullName: string
  email: string
  emailVerified: boolean
}

export type Workspace = {
  id: string
  name: string
  slug: string
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

export type DatabaseMetricPoint = {
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
}

export type DatabaseMetricsRange = "1h" | "6h" | "24h" | "7d" | "30d"

export type DatabaseMetrics = {
  provider: string
  systemMetricsAvailable: boolean
  systemMetricsMessage: string | null
  sampleIntervalSeconds: number
  retentionSeconds: number
  range: DatabaseMetricsRange
  fromTimestamp: number
  toTimestamp: number
  resolutionSeconds: number
  points: DatabaseMetricPoint[]
}

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
