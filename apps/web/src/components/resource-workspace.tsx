import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type ReactNode,
} from "react"
import {
  ActivityIcon,
  Clock3Icon,
  CopyIcon,
  DatabaseIcon,
  FileTextIcon,
  GitBranchIcon,
  HistoryIcon,
  PlusIcon,
  RefreshCwIcon,
  SearchIcon,
  ServerIcon,
  Settings2Icon,
  Table2Icon,
  TerminalIcon,
  XIcon,
} from "lucide-react"
import { cn } from "cn"

import { Button } from "@/components/ui/button"
import { AppServiceDeploymentLogs } from "@/components/app-service-deployment-logs"
import { AppServiceRuntimeLogs } from "@/components/app-service-runtime-logs"
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { ApiError } from "@/lib/api"
import {
  createDatabaseTable,
  executeDatabaseQuery,
  getAppServiceMetrics,
  getDatabaseConfig,
  getDatabaseMetrics,
  getDatabaseStats,
  getDatabaseTableData,
  listDatabaseTables,
  listPostgresResources,
  retryPostgresResource,
  updateAppService,
  updateAppServiceAutoDeploy,
  updateAppServiceDatabase,
  updateAppServicePublicAccess,
} from "@/lib/resources"
import type {
  DatabaseConfig,
  DatabaseMetricsRange,
  DatabaseStats,
  DatabaseTable,
  DatabaseTableData,
  DatabaseQueryResult,
  AppService,
  ResourceMetricPoint,
  ResourceMetrics,
  PostgresResource,
  RedisResource,
} from "@/lib/types"

import "./resource-workspace.css"

type ResourceWorkspaceNode = {
  id: string
  title: string
  subtitle?: string
  type: string
  volume: string
  status: string
  resource?: PostgresResource | AppService | RedisResource
}

type ResourceWorkspaceTab =
  | "deployments"
  | "database"
  | "backups"
  | "variables"
  | "metrics"
  | "console"
  | "settings"

const resourceTabs: Array<{
  id: ResourceWorkspaceTab
  label: string
}> = [
  { id: "deployments", label: "Deployments" },
  { id: "database", label: "Database" },
  { id: "backups", label: "Backups" },
  { id: "variables", label: "Variables" },
  { id: "metrics", label: "Metrics" },
  { id: "console", label: "Console" },
  { id: "settings", label: "Settings" },
]

type ResourceWorkspaceProps = {
  node: ResourceWorkspaceNode
  environment: string
  workspaceSlug: string
  projectSlug: string
  onClose: () => void
  onCopyConnectionString: (value: string) => void
  copiedConnectionString: boolean
  onToast: (message: string) => void
  onOpenLogs: () => void
  onAppServiceUpdated?: (resource: AppService) => void
  onPostgresUpdated?: (resource: PostgresResource) => void
}

const DEFAULT_QUERY = "SELECT 1"
const TABLE_SEARCH_DEBOUNCE_MS = 250
const METRICS_REFRESH_MS = 5_000
const METRIC_RANGE_OPTIONS: Array<{
  value: DatabaseMetricsRange
  label: string
}> = [
  { value: "1h", label: "Last hour" },
  { value: "6h", label: "Last 6 hours" },
  { value: "24h", label: "Last 24 hours" },
  { value: "7d", label: "Last 7 days" },
  { value: "30d", label: "Last 30 days" },
]

export function ResourceWorkspace({
  node,
  environment,
  workspaceSlug,
  projectSlug,
  onClose,
  onCopyConnectionString,
  copiedConnectionString,
  onToast,
  onOpenLogs,
  onAppServiceUpdated,
  onPostgresUpdated,
}: ResourceWorkspaceProps) {
  const closeButtonRef = useRef<HTMLButtonElement>(null)
  const [activeTab, setActiveTab] =
    useState<ResourceWorkspaceTab>("deployments")

  useEffect(() => {
    closeButtonRef.current?.focus()
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = "hidden"

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose()
      }
    }

    document.addEventListener("keydown", handleKeyDown)
    return () => {
      document.body.style.overflow = previousOverflow
      document.removeEventListener("keydown", handleKeyDown)
    }
  }, [onClose])

  function handleTabKeyDown(
    event: React.KeyboardEvent<HTMLButtonElement>,
    tab: ResourceWorkspaceTab
  ) {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault()
      setActiveTab(tab)
      return
    }
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") {
      return
    }
    event.preventDefault()
    const currentIndex = resourceTabs.findIndex((item) => item.id === tab)
    const direction = event.key === "ArrowRight" ? 1 : -1
    const nextIndex =
      (currentIndex + direction + resourceTabs.length) % resourceTabs.length
    document
      .getElementById(`resource-tab-${resourceTabs[nextIndex].id}`)
      ?.focus()
    setActiveTab(resourceTabs[nextIndex].id)
  }

  const connectionString =
    node.resource?.resourceType === "postgres" ||
    node.resource?.resourceType === "redis"
      ? (node.resource.connectionString ?? undefined)
      : undefined

  return (
    <div
      className="resource-workspace-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="resource-workspace-title"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) {
          onClose()
        }
      }}
    >
      <div className="resource-workspace-sheet">
        <div className="resource-workspace-head">
          <div className="resource-workspace-identity">
            <span
              className={cn(
                "resource-workspace-logo",
                node.resource?.resourceType === "postgres" ? "postgres" : "project"
              )}
              aria-hidden="true"
            >
              {node.id === "postgres" ? <DatabaseIcon /> : <GitBranchIcon />}
            </span>
            <div className="resource-workspace-title-group">
              <h2 id="resource-workspace-title">{node.title}</h2>
              <p>
                {node.type} · {environment}
              </p>
            </div>
          </div>
          <button
            ref={closeButtonRef}
            className="resource-workspace-close"
            type="button"
            aria-label="Close resource workspace"
            onClick={onClose}
          >
            <XIcon aria-hidden="true" />
          </button>
        </div>

        <div
          className="resource-workspace-tabs"
          role="tablist"
          aria-label="Resource sections"
        >
          {resourceTabs.map((tab) => (
            <button
              key={tab.id}
              id={`resource-tab-${tab.id}`}
              className="resource-workspace-tab"
              type="button"
              role="tab"
              aria-selected={activeTab === tab.id}
              aria-controls={`resource-pane-${tab.id}`}
              tabIndex={activeTab === tab.id ? 0 : -1}
              onClick={() => setActiveTab(tab.id)}
              onKeyDown={(event) => handleTabKeyDown(event, tab.id)}
            >
              {tab.label}
            </button>
          ))}
        </div>

        <div className="resource-workspace-body">
          {activeTab === "deployments" && (
            <DeploymentsPane
              node={node}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              onToast={onToast}
              onOpenLogs={onOpenLogs}
              onOpenRuntimeLogs={() => setActiveTab("console")}
              onPostgresUpdated={onPostgresUpdated}
            />
          )}
          {activeTab === "database" && (
            <DatabasePane
              node={node}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              connectionString={connectionString}
              copiedConnectionString={copiedConnectionString}
              onCopyConnectionString={onCopyConnectionString}
              onToast={onToast}
              onPostgresUpdated={onPostgresUpdated}
            />
          )}
          {activeTab === "backups" && <BackupsPane onToast={onToast} />}
          {activeTab === "variables" && (
            <VariablesPane node={node} onToast={onToast} />
          )}
          {activeTab === "metrics" && (
            <MetricsPane
              node={node}
              projectSlug={projectSlug}
              workspaceSlug={workspaceSlug}
              onToast={onToast}
            />
          )}
          {activeTab === "console" && (
            <ConsolePane
              node={node}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              onToast={onToast}
            />
          )}
          {activeTab === "settings" && (
            <SettingsPane
              node={node}
              environment={environment}
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              onToast={onToast}
              onAppServiceUpdated={onAppServiceUpdated}
            />
          )}
        </div>
      </div>
    </div>
  )
}

function DeploymentsPane({
  node,
  workspaceSlug,
  projectSlug,
  onToast,
  onOpenLogs,
  onOpenRuntimeLogs,
  onPostgresUpdated,
}: {
  node: ResourceWorkspaceNode
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
  onOpenLogs: () => void
  onOpenRuntimeLogs: () => void
  onPostgresUpdated?: (resource: PostgresResource) => void
}) {
  const isReady =
    node.resource?.status === "ready" ||
    (node.id === "project" && !node.resource)
  const isError = node.resource?.status === "error"
  const status = isReady
    ? "ACTIVE"
    : isError
      ? "ERROR"
      : (node.resource?.status.toUpperCase() ?? "NOT DEPLOYED")
  const appService =
    node.resource?.resourceType === "app" ? node.resource : undefined
  const postgresResource =
    node.resource?.resourceType === "postgres" ? node.resource : undefined

  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-deployments"
      role="tabpanel"
      aria-labelledby="resource-tab-deployments"
    >
      <div className="resource-workspace-banner">
        <p>
          {node.id === "postgres"
            ? isError
              ? "This database could not be scheduled on the Kubernetes cluster. Retry after capacity is available."
              : "This database is isolated to the current project and ready for application connections."
            : appService
              ? "This Docker image runs as the application entry point for the current project."
              : "Deploy a Docker image to make this project available as an application service."}
        </p>
        <div className="resource-workspace-banner-actions">
          <button
            className="resource-workspace-ghost-link"
            type="button"
            onClick={() => onToast("Support request started")}
          >
            Something&apos;s wrong
          </button>
          <button
            className="resource-workspace-solid-button"
            type="button"
            onClick={() => onToast("Resource health confirmed")}
          >
            Looks good
          </button>
        </div>
      </div>
      <div className="resource-workspace-meta-row">
        <span>
          {node.id === "postgres" ? "Private database" : "Docker app service"}
        </span>
        <span>
          {postgresResource
            ? `${postgresResource.host} · ${postgresResource.port}`
            : (appService?.serviceUrl ??
              appService?.publicDomain ??
              "No container deployed")}
        </span>
      </div>
      <article
        className={cn(
          "resource-workspace-deploy-card",
          isReady ? "active" : "pending"
        )}
      >
        <div className="resource-workspace-deploy-top">
          <div className="resource-workspace-deploy-identity">
            <span
              className={cn(
                "resource-workspace-status-pill",
                isReady ? "active" : "pending"
              )}
            >
              {status}
            </span>
            <div>
              <strong>
                {postgresResource
                  ? `PostgreSQL · ${postgresResource.databaseName}`
                  : appService
                    ? `${appService.imageSource === "github" ? "Private GitHub" : "Public"} · ${appService.image}`
                    : node.title}
              </strong>
              <div className="resource-workspace-muted">
                {isReady
                  ? appService
                    ? "Container is running and ready for traffic"
                    : "Ready to accept connections"
                  : isError
                    ? (node.resource?.errorMessage ??
                      (node.id === "postgres"
                        ? "The cluster has no schedulable capacity."
                        : "Deployment failed"))
                    : "Deploy an image to start this service"}
              </div>
            </div>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={appService ? onOpenRuntimeLogs : onOpenLogs}
          >
            <FileTextIcon data-icon="inline-start" />
            {appService ? "View runtime logs" : "View logs"}
          </Button>
        </div>
        <div className="resource-workspace-deploy-bottom">
          <span>
            {isReady
              ? appService?.serviceUrl
                ? `Service URL · ${appService.serviceUrl}`
                : "Deployment successful"
              : isError
                ? "Provisioning failed — deploy again when capacity is available"
                : appService?.publicDomain
                  ? `Domain assigned · ${appService.publicDomain}`
                  : "Deployment not started"}
          </span>
          {isError && postgresResource && (
            <PostgresRetryButton
              workspaceSlug={workspaceSlug}
              projectSlug={projectSlug}
              resource={postgresResource}
              onToast={onToast}
              onUpdated={onPostgresUpdated}
            />
          )}
          {isReady && appService?.serviceUrl && (
            <a
              className="resource-workspace-ghost-link"
              href={appService.serviceUrl}
              target="_blank"
              rel="noreferrer"
            >
              Open service
            </a>
          )}
        </div>
      </article>
      {appService?.deployment && (
        <AppServiceDeploymentLogs deployment={appService.deployment} compact />
      )}
      {appService?.databaseConnection && (
        <article className="resource-workspace-private-link" role="status">
          <span
            className="resource-workspace-private-link-icon"
            aria-hidden="true"
          >
            <DatabaseIcon />
          </span>
          <div>
            <strong>Postgres connection assigned</strong>
            <span>
              {appService.databaseConnection.name} via{" "}
              <code>
                {appService.databaseConnection.host}:
                {appService.databaseConnection.port}
              </code>
            </span>
            <small>
              {appService.databaseConnection.environmentVariables.length}{" "}
              private variables assigned on the project network
            </small>
          </div>
          <span className="resource-workspace-private-link-status">
            ASSIGNED
          </span>
        </article>
      )}
      <div className="resource-workspace-history-head">
        <span>HISTORY</span>
        <button
          className="resource-workspace-ghost-link"
          type="button"
          onClick={() => onToast("Deployment history is up to date")}
        >
          Hide skipped
        </button>
      </div>
      <article className="resource-workspace-history-card">
        <HistoryIcon aria-hidden="true" />
        <div>
          <strong>
            {postgresResource
              ? isError
                ? "Database provisioning failed"
                : "Database provisioned"
              : appService
                ? "App service deployed"
                : "Project created"}
          </strong>
          <div className="resource-workspace-muted">
            Current environment · Knotree Cloud
          </div>
        </div>
        <span className="resource-workspace-history-status">
          {isReady ? "READY" : status}
        </span>
      </article>
    </section>
  )
}

function PostgresRetryButton({
  workspaceSlug,
  projectSlug,
  resource,
  onToast,
  onUpdated,
}: {
  workspaceSlug: string
  projectSlug: string
  resource: PostgresResource
  onToast: (message: string) => void
  onUpdated?: (resource: PostgresResource) => void
}) {
  const [retrying, setRetrying] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleRetry() {
    if (retrying || resource.status === "provisioning") {
      return
    }
    setRetrying(true)
    setError(null)
    try {
      const nextResource = await retryPostgresResource(
        workspaceSlug,
        projectSlug,
        resource.id
      )
      onUpdated?.(nextResource)
      onToast("Postgres redeploy started")
    } catch (requestError) {
      const message =
        requestError instanceof ApiError
          ? requestError.message
          : "Postgres redeploy failed. Please try again."
      setError(message)
    } finally {
      setRetrying(false)
    }
  }

  return (
    <span className="resource-workspace-retry-action">
      <Button
        type="button"
        variant="outline"
        size="sm"
        disabled={retrying || resource.status === "provisioning"}
        aria-label="Deploy PostgreSQL again"
        onClick={() => {
          void handleRetry()
        }}
      >
        <RefreshCwIcon
          data-icon="inline-start"
          className={cn(retrying && "animate-spin")}
        />
        {retrying ? "Deploying…" : "Deploy again"}
      </Button>
      {error && (
        <span className="resource-workspace-retry-error" role="alert">
          {error}
        </span>
      )}
    </span>
  )
}

function DatabasePane({
  node,
  workspaceSlug,
  projectSlug,
  connectionString,
  copiedConnectionString,
  onCopyConnectionString,
  onToast,
  onPostgresUpdated,
}: {
  node: ResourceWorkspaceNode
  workspaceSlug: string
  projectSlug: string
  connectionString?: string
  copiedConnectionString: boolean
  onCopyConnectionString: (value: string) => void
  onToast: (message: string) => void
  onPostgresUpdated?: (resource: PostgresResource) => void
}) {
  const [view, setView] = useState<"data" | "stats" | "config">("data")
  const [search, setSearch] = useState("")
  const [tableSearch, setTableSearch] = useState("")
  const [tables, setTables] = useState<DatabaseTable[]>([])
  const [selectedTable, setSelectedTable] = useState<DatabaseTable | null>(null)
  const [tableOffset, setTableOffset] = useState(0)
  const [tableData, setTableData] = useState<DatabaseTableData | null>(null)
  const [stats, setStats] = useState<DatabaseStats | null>(null)
  const [config, setConfig] = useState<DatabaseConfig[]>([])
  const [query, setQuery] = useState(DEFAULT_QUERY)
  const [queryResult, setQueryResult] = useState<DatabaseQueryResult | null>(
    null
  )
  const [loading, setLoading] = useState(true)
  const [queryRunning, setQueryRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const [tablesRefreshKey, setTablesRefreshKey] = useState(0)
  const tableRequestId = useRef(0)
  const resourceId = node.resource?.id
  const isReady = node.resource?.status === "ready"
  const isError = node.resource?.status === "error"
  const selectedTableName = selectedTable?.tableName
  const selectedTableSchemaName = selectedTable?.schemaName

  const loadTables = useCallback(async () => {
    if (!resourceId || !isReady) {
      return
    }
    const requestId = tableRequestId.current + 1
    tableRequestId.current = requestId
    try {
      const nextTables = await listDatabaseTables(
        workspaceSlug,
        projectSlug,
        resourceId,
        tableSearch
      )
      if (requestId !== tableRequestId.current) {
        return
      }
      setTables(nextTables)
      setSelectedTable((current) =>
        current &&
        nextTables.some(
          (table) =>
            table.schemaName === current.schemaName &&
            table.tableName === current.tableName
        )
          ? current
          : null
      )
      setQuery((current) =>
        current === DEFAULT_QUERY && nextTables[0]
          ? queryForTable(nextTables[0])
          : current
      )
    } catch (requestError) {
      if (requestId === tableRequestId.current) {
        setError(databaseErrorMessage(requestError))
      }
    } finally {
      if (requestId === tableRequestId.current) {
        setLoading(false)
      }
    }
  }, [isReady, projectSlug, resourceId, tableSearch, workspaceSlug])

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      setTableSearch(search.trim())
    }, TABLE_SEARCH_DEBOUNCE_MS)
    return () => window.clearTimeout(timeoutId)
  }, [search])

  useEffect(() => {
    if (!resourceId || !isReady) {
      return
    }
    const timeoutId = window.setTimeout(() => {
      void loadTables()
    }, 0)
    return () => window.clearTimeout(timeoutId)
  }, [isReady, loadTables, resourceId, tablesRefreshKey])

  useEffect(() => {
    if (
      !resourceId ||
      !isReady ||
      !selectedTableName ||
      !selectedTableSchemaName ||
      view !== "data"
    ) {
      return
    }
    let active = true
    void getDatabaseTableData(
      workspaceSlug,
      projectSlug,
      resourceId,
      selectedTableName,
      selectedTableSchemaName,
      50,
      tableOffset
    )
      .then((nextData) => {
        if (active) {
          setTableData(nextData)
        }
      })
      .catch((requestError: unknown) => {
        if (active) {
          setError(databaseErrorMessage(requestError))
          setTableData(null)
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false)
        }
      })
    return () => {
      active = false
    }
  }, [
    isReady,
    projectSlug,
    resourceId,
    selectedTableName,
    selectedTableSchemaName,
    tableOffset,
    view,
    workspaceSlug,
  ])

  useEffect(() => {
    if (!resourceId || !isReady || view !== "stats") {
      return
    }
    let active = true
    void getDatabaseStats(workspaceSlug, projectSlug, resourceId)
      .then((nextStats) => {
        if (active) {
          setStats(nextStats)
        }
      })
      .catch((requestError: unknown) => {
        if (active) {
          setError(databaseErrorMessage(requestError))
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false)
        }
      })
    return () => {
      active = false
    }
  }, [isReady, projectSlug, resourceId, view, workspaceSlug])

  useEffect(() => {
    if (!resourceId || !isReady || view !== "config") {
      return
    }
    let active = true
    void getDatabaseConfig(workspaceSlug, projectSlug, resourceId)
      .then((nextConfig) => {
        if (active) {
          setConfig(nextConfig)
        }
      })
      .catch((requestError: unknown) => {
        if (active) {
          setError(databaseErrorMessage(requestError))
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false)
        }
      })
    return () => {
      active = false
    }
  }, [isReady, projectSlug, resourceId, view, workspaceSlug])

  async function handleRunQuery() {
    if (!resourceId || !isReady || !query.trim()) {
      return
    }
    setQueryRunning(true)
    setError(null)
    setQueryResult(null)
    try {
      const result = await executeDatabaseQuery(
        workspaceSlug,
        projectSlug,
        resourceId,
        query
      )
      setQueryResult(result)
      onToast(
        result.affectedRows > 0
          ? `Query completed · ${result.affectedRows} row(s) affected`
          : `Query completed · ${result.rowCount} row(s)`
      )
    } catch (requestError) {
      setError(databaseErrorMessage(requestError))
    } finally {
      setQueryRunning(false)
    }
  }

  function handleCreatedTable() {
    setCreateOpen(false)
    setLoading(true)
    setError(null)
    setSearch("")
    setSelectedTable(null)
    setTableOffset(0)
    setTableData(null)
    setTableSearch("")
    setTablesRefreshKey((current) => current + 1)
    onToast("Table created")
  }

  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-database"
      role="tabpanel"
      aria-labelledby="resource-tab-database"
    >
      <div
        className="resource-workspace-sub-tabs"
        role="tablist"
        aria-label="Database views"
      >
        {(["data", "stats", "config"] as const).map((item) => (
          <button
            key={item}
            className="resource-workspace-sub-tab"
            type="button"
            role="tab"
            aria-selected={view === item}
            onClick={() => {
              setView(item)
              setLoading(true)
              setError(null)
            }}
          >
            {item[0].toUpperCase() + item.slice(1)}
          </button>
        ))}
        <span className="resource-workspace-sub-tabs-spacer" />
        {connectionString ? (
          <button
            className="resource-workspace-ghost-link"
            type="button"
            onClick={() => onCopyConnectionString(connectionString)}
          >
            {copiedConnectionString ? "Copied" : "Connect"}
          </button>
        ) : (
          <span className="resource-workspace-muted">Connect when ready</span>
        )}
      </div>

      {node.id !== "postgres" || !resourceId ? (
        <ResourceEmptyState
          icon={<DatabaseIcon aria-hidden="true" />}
          title="No database resource"
          description="Attach a PostgreSQL resource to this project to manage its database."
        />
      ) : !isReady ? (
        <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title={
            isError
              ? "Database provisioning failed"
              : "Database is provisioning"
          }
          description={
            isError
              ? (node.resource?.errorMessage ??
                "The dedicated PostgreSQL cluster could not be scheduled. Retry when capacity is available.")
              : "Database management becomes available as soon as the dedicated PostgreSQL cluster is ready."
          }
          action={
            isError && node.resource?.resourceType === "postgres" ? (
              <PostgresRetryButton
                workspaceSlug={workspaceSlug}
                projectSlug={projectSlug}
                resource={node.resource}
                onToast={onToast}
                onUpdated={onPostgresUpdated}
              />
            ) : undefined
          }
        />
      ) : (
        <>
          {error && (
            <div className="resource-workspace-error" role="alert">
              {error}
            </div>
          )}

          {view === "data" && (
            <div>
              <div className="resource-workspace-table-toolbar">
                <div className="resource-workspace-search-field">
                  <strong>Tables</strong>
                  <div className="resource-workspace-input-wrap">
                    <SearchIcon aria-hidden="true" />
                    <Input
                      aria-label="Search tables"
                      value={search}
                      placeholder="Search tables"
                      onChange={(event) => {
                        setSearch(event.target.value)
                        setLoading(true)
                        setError(null)
                      }}
                    />
                  </div>
                </div>
                <div className="resource-workspace-toolbar-actions">
                  <Button
                    type="button"
                    variant="outline"
                    size="icon-sm"
                    aria-label="Refresh tables"
                    onClick={() => {
                      setLoading(true)
                      setError(null)
                      setTablesRefreshKey((current) => current + 1)
                    }}
                    disabled={loading}
                  >
                    <RefreshCwIcon />
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    className="resource-workspace-accent-button"
                    onClick={() => setCreateOpen(true)}
                  >
                    <PlusIcon data-icon="inline-start" />
                    New Table
                  </Button>
                </div>
              </div>
              <div className="resource-workspace-query-editor">
                <Textarea
                  aria-label="SQL query"
                  rows={2}
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  spellCheck={false}
                />
                <Button
                  type="button"
                  size="sm"
                  onClick={() => void handleRunQuery()}
                  disabled={queryRunning || !query.trim()}
                >
                  <TerminalIcon data-icon="inline-start" />
                  {queryRunning ? "Running…" : "Run query"}
                </Button>
              </div>
              {queryResult && <DatabaseQueryResultView result={queryResult} />}
              {selectedTable ? (
                tableData ? (
                  <DatabaseTableView
                    data={tableData}
                    loading={loading}
                    onPageChange={setTableOffset}
                    onBack={() => {
                      setSelectedTable(null)
                      setTableData(null)
                    }}
                  />
                ) : (
                  <ResourceEmptyState
                    icon={<Table2Icon aria-hidden="true" />}
                    title={loading ? "Loading rows…" : "No table data"}
                    description="The selected table data will appear here when the database responds."
                  />
                )
              ) : tables.length > 0 ? (
                <div className="resource-workspace-table-grid">
                  {tables.map((table) => (
                    <button
                      key={`${table.schemaName}.${table.tableName}`}
                      type="button"
                      className="resource-workspace-table-card"
                      onClick={() => {
                        setQueryResult(null)
                        setLoading(true)
                        setError(null)
                        setSelectedTable(table)
                        setTableOffset(0)
                      }}
                    >
                      <Table2Icon aria-hidden="true" />
                      <span>{table.tableName}</span>
                      <small>
                        {table.schemaName} ·{" "}
                        {formatRowCount(table.estimatedRows)} rows
                      </small>
                    </button>
                  ))}
                </div>
              ) : (
                <ResourceEmptyState
                  icon={<Table2Icon aria-hidden="true" />}
                  title={
                    loading
                      ? "Loading tables…"
                      : search
                        ? "No tables match"
                        : "No tables yet"
                  }
                  description={
                    search
                      ? "Try a different table name."
                      : "Create a table here or connect your application to this dedicated PostgreSQL cluster."
                  }
                />
              )}
            </div>
          )}
          {view === "stats" &&
            (stats ? (
              <DatabaseStatsView stats={stats} />
            ) : (
              <ResourceEmptyState
                icon={<ActivityIcon aria-hidden="true" />}
                title={loading ? "Loading stats…" : "No stats available"}
                description="Live statistics are read directly from this project database."
              />
            ))}
          {view === "config" &&
            (config.length > 0 ? (
              <DatabaseConfigView config={config} />
            ) : (
              <ResourceEmptyState
                icon={<Settings2Icon aria-hidden="true" />}
                title={loading ? "Loading config…" : "No config available"}
                description="The live PostgreSQL settings are read from this project database."
              />
            ))}
          {createOpen && (
            <CreateTableForm
              onCancel={() => setCreateOpen(false)}
              onCreated={handleCreatedTable}
              onError={setError}
              projectSlug={projectSlug}
              resourceId={resourceId}
              schemaName="public"
              workspaceSlug={workspaceSlug}
            />
          )}
        </>
      )}
    </section>
  )
}

function DatabaseTableView({
  data,
  loading,
  onPageChange,
  onBack,
}: {
  data: DatabaseTableData
  loading: boolean
  onPageChange: (offset: number) => void
  onBack: () => void
}) {
  return (
    <div className="resource-workspace-data-view">
      <div className="resource-workspace-data-head">
        <Button type="button" variant="outline" size="sm" onClick={onBack}>
          Back to tables
        </Button>
        <strong>
          {data.schemaName}.{data.tableName}
        </strong>
        <span className="resource-workspace-muted">
          {loading ? "Refreshing…" : `${data.rowCount} rows`}
        </span>
      </div>
      {data.columns.length === 0 ? (
        <ResourceEmptyState
          icon={<Table2Icon aria-hidden="true" />}
          title="No columns"
          description="This table has no visible columns."
        />
      ) : (
        <div className="resource-workspace-data-table-wrap">
          <table className="resource-workspace-data-table">
            <thead>
              <tr>
                {data.columns.map((column) => (
                  <th key={column.name} scope="col">
                    <span>{column.name}</span>
                    <small>{column.dataType}</small>
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {data.rows.length === 0 ? (
                <tr>
                  <td colSpan={data.columns.length}>No rows yet.</td>
                </tr>
              ) : (
                data.rows.map((row, rowIndex) => (
                  <tr key={rowIndex}>
                    {data.columns.map((column) => (
                      <td key={column.name}>{formatCell(row[column.name])}</td>
                    ))}
                  </tr>
                ))
              )}
            </tbody>
          </table>
        </div>
      )}
      <div className="resource-workspace-pagination">
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={data.offset === 0 || loading}
          onClick={() => onPageChange(Math.max(0, data.offset - data.limit))}
        >
          Previous
        </Button>
        <span className="resource-workspace-muted">
          Rows {data.rowCount === 0 ? 0 : data.offset + 1}–
          {data.offset + data.rowCount}
        </span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={loading || data.rowCount < data.limit}
          onClick={() => onPageChange(data.offset + data.limit)}
        >
          Next
        </Button>
      </div>
    </div>
  )
}

function DatabaseQueryResultView({ result }: { result: DatabaseQueryResult }) {
  if (result.columns.length === 0) {
    return (
      <div className="resource-workspace-query-result" role="status">
        Query completed in {result.durationMs}ms · {result.affectedRows} row(s)
        affected.
      </div>
    )
  }
  return (
    <div className="resource-workspace-query-result">
      <div className="resource-workspace-data-head">
        <strong>Query result</strong>
        <span className="resource-workspace-muted">
          {result.rowCount} rows · {result.durationMs}ms
          {result.truncated ? " · result truncated" : ""}
        </span>
      </div>
      <div className="resource-workspace-data-table-wrap">
        <table className="resource-workspace-data-table">
          <thead>
            <tr>
              {result.columns.map((column) => (
                <th key={column} scope="col">
                  {column}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {result.rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                {result.columns.map((column) => (
                  <td key={column}>{formatCell(row[column])}</td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function DatabaseStatsView({ stats }: { stats: DatabaseStats }) {
  return (
    <div className="resource-workspace-db-cards">
      {[
        ["Database", stats.databaseName],
        ["Storage", formatBytes(stats.sizeBytes)],
        ["Connections", `${stats.connections} / ${stats.maxConnections}`],
        ["Tables", String(stats.tableCount)],
        ["Estimated rows", formatRowCount(stats.estimatedRows)],
      ].map(([label, value]) => (
        <article key={label} className="resource-workspace-db-card">
          <span>{label}</span>
          <strong>{value}</strong>
        </article>
      ))}
    </div>
  )
}

function DatabaseConfigView({ config }: { config: DatabaseConfig[] }) {
  return (
    <div className="resource-workspace-config-list">
      {config.map((item) => (
        <article key={item.name} className="resource-workspace-config-row">
          <div>
            <strong>{item.name}</strong>
            <p>{item.description}</p>
          </div>
          <code>
            {item.setting}
            {item.unit ? ` ${item.unit}` : ""}
          </code>
        </article>
      ))}
    </div>
  )
}

function CreateTableForm({
  workspaceSlug,
  projectSlug,
  resourceId,
  schemaName,
  onCancel,
  onCreated,
  onError,
}: {
  workspaceSlug: string
  projectSlug: string
  resourceId: string
  schemaName: string
  onCancel: () => void
  onCreated: () => void
  onError: (message: string) => void
}) {
  const [name, setName] = useState("")
  const [columns, setColumns] = useState([
    { name: "id", dataType: "bigint", primaryKey: true, nullable: false },
  ])
  const [saving, setSaving] = useState(false)

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setSaving(true)
    try {
      await createDatabaseTable(workspaceSlug, projectSlug, resourceId, {
        name,
        schema: schemaName,
        columns,
      })
      onCreated()
    } catch (requestError) {
      onError(databaseErrorMessage(requestError))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="resource-workspace-modal-backdrop">
      <form
        className="resource-workspace-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="new-table-title"
        onSubmit={(event) => void handleSubmit(event)}
      >
        <div className="resource-workspace-modal-head">
          <div>
            <h3 id="new-table-title">New table</h3>
            <p>Create a table in the {schemaName} schema.</p>
          </div>
          <button
            type="button"
            className="resource-workspace-close"
            aria-label="Close new table"
            onClick={onCancel}
          >
            <XIcon aria-hidden="true" />
          </button>
        </div>
        <label className="resource-workspace-form-field">
          Table name
          <Input
            value={name}
            onChange={(event) => setName(event.target.value)}
            required
            autoFocus
          />
        </label>
        <div className="resource-workspace-form-section">
          <div className="resource-workspace-form-section-head">
            <strong>Columns</strong>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() =>
                setColumns((current) => [
                  ...current,
                  {
                    name: `column_${current.length + 1}`,
                    dataType: "text",
                    primaryKey: false,
                    nullable: true,
                  },
                ])
              }
              disabled={columns.length >= 50}
            >
              <PlusIcon data-icon="inline-start" /> Add column
            </Button>
          </div>
          {columns.map((column, index) => (
            <div
              className="resource-workspace-column-row"
              key={`${index}-${column.name}`}
            >
              <Input
                aria-label={`Column ${index + 1} name`}
                value={column.name}
                onChange={(event) =>
                  setColumns((current) =>
                    current.map((item, itemIndex) =>
                      itemIndex === index
                        ? { ...item, name: event.target.value }
                        : item
                    )
                  )
                }
                required
              />
              <select
                aria-label={`Column ${index + 1} type`}
                value={column.dataType}
                onChange={(event) =>
                  setColumns((current) =>
                    current.map((item, itemIndex) =>
                      itemIndex === index
                        ? { ...item, dataType: event.target.value }
                        : item
                    )
                  )
                }
              >
                {[
                  "text",
                  "bigint",
                  "integer",
                  "boolean",
                  "numeric",
                  "date",
                  "timestamptz",
                  "uuid",
                  "jsonb",
                  "bytea",
                ].map((type) => (
                  <option key={type} value={type}>
                    {type}
                  </option>
                ))}
              </select>
              <label className="resource-workspace-checkbox">
                <input
                  type="checkbox"
                  checked={column.nullable}
                  disabled={column.primaryKey}
                  onChange={(event) =>
                    setColumns((current) =>
                      current.map((item, itemIndex) =>
                        itemIndex === index
                          ? { ...item, nullable: event.target.checked }
                          : item
                      )
                    )
                  }
                />
                Nullable
              </label>
              <label className="resource-workspace-checkbox">
                <input
                  type="checkbox"
                  checked={column.primaryKey}
                  onChange={(event) =>
                    setColumns((current) =>
                      current.map((item, itemIndex) =>
                        itemIndex === index
                          ? {
                              ...item,
                              primaryKey: event.target.checked,
                              nullable: event.target.checked
                                ? false
                                : item.nullable,
                            }
                          : item
                      )
                    )
                  }
                />
                PK
              </label>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-label={`Remove column ${index + 1}`}
                onClick={() =>
                  setColumns((current) =>
                    current.filter((_, itemIndex) => itemIndex !== index)
                  )
                }
                disabled={columns.length === 1}
              >
                <XIcon />
              </Button>
            </div>
          ))}
        </div>
        <div className="resource-workspace-modal-actions">
          <Button type="button" variant="outline" onClick={onCancel}>
            Cancel
          </Button>
          <Button type="submit" disabled={saving || !name.trim()}>
            {saving ? "Creating…" : "Create table"}
          </Button>
        </div>
      </form>
    </div>
  )
}

function databaseErrorMessage(error: unknown) {
  return error instanceof ApiError
    ? error.message
    : "The database operation failed. Please try again."
}

function queryForTable(table: DatabaseTable) {
  return `SELECT * FROM ${quoteSqlIdentifier(table.schemaName)}.${quoteSqlIdentifier(table.tableName)} LIMIT 50`
}

function quoteSqlIdentifier(identifier: string) {
  return `"${identifier.replaceAll('"', '""')}"`
}

function formatCell(value: unknown) {
  if (value === null || value === undefined) {
    return "NULL"
  }
  if (typeof value === "object") {
    return JSON.stringify(value)
  }
  return String(value)
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024)
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

function formatRowCount(value: number) {
  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value)
}

function BackupsPane({ onToast }: { onToast: (message: string) => void }) {
  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-backups"
      role="tabpanel"
      aria-labelledby="resource-tab-backups"
    >
      <div className="resource-workspace-hint-banner">
        <p>Backups and point-in-time recovery are available on the Pro plan.</p>
        <button
          className="resource-workspace-ghost-link"
          type="button"
          onClick={() => onToast("Plan details opened")}
        >
          More information
        </button>
      </div>
      <ResourceEmptyState
        icon={<Clock3Icon aria-hidden="true" />}
        title="No backups"
        description="This database does not have any backups yet."
      />
    </section>
  )
}

function VariablesPane({
  node,
  onToast,
}: {
  node: ResourceWorkspaceNode
  onToast: (message: string) => void
}) {
  const [search, setSearch] = useState("")
  const appDatabaseConnection =
    node.resource?.resourceType === "app"
      ? node.resource.databaseConnection
      : undefined
  const variables =
    node.resource?.resourceType === "postgres"
      ? [
          ["DATABASE_URL", "postgres://••••••••"],
          ["PGDATABASE", node.resource.databaseName],
          ["PGHOST", node.resource.host],
          ["PGPORT", String(node.resource.port)],
          ["PGUSER", node.resource.username],
        ]
      : node.resource?.resourceType === "app"
        ? [
            ["APP_IMAGE", node.resource.image],
            ["APP_IMAGE_SOURCE", node.resource.imageSource],
            ["APP_PORT", String(node.resource.appPort)],
            ["APP_SERVICE_URL", node.resource.serviceUrl ?? "pending"],
            ...(appDatabaseConnection
              ? [
                  [
                    "DATABASE_URL",
                    "postgres://••••••••@" +
                      appDatabaseConnection.host +
                      ":" +
                      appDatabaseConnection.port +
                      "/" +
                      appDatabaseConnection.databaseName,
                  ],
                  ["PGHOST", appDatabaseConnection.host],
                  ["PGPORT", String(appDatabaseConnection.port)],
                  ["PGDATABASE", appDatabaseConnection.databaseName],
                  ["PGUSER", appDatabaseConnection.username],
                  ["PGPASSWORD", "••••••••"],
                ]
              : []),
          ]
        : [["APP_ENV", "development"]]
  const filteredVariables = variables.filter(([name]) =>
    name.toLowerCase().includes(search.toLowerCase())
  )

  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-variables"
      role="tabpanel"
      aria-labelledby="resource-tab-variables"
    >
      <div className="resource-workspace-toolbar-row">
        <div className="resource-workspace-search-field">
          <strong>{variables.length} Service Variables</strong>
          <div className="resource-workspace-input-wrap">
            <SearchIcon aria-hidden="true" />
            <Input
              aria-label="Search variables"
              value={search}
              placeholder="Search variables"
              onChange={(event) => setSearch(event.target.value)}
            />
          </div>
        </div>
        <div className="resource-workspace-toolbar-actions">
          <button
            className="resource-workspace-ghost-link"
            type="button"
            onClick={() => onToast("Shared variables are coming soon")}
          >
            Shared Variable
          </button>
          <button
            className="resource-workspace-ghost-link"
            type="button"
            onClick={() => onToast("Raw editor is coming soon")}
          >
            Raw Editor
          </button>
          <Button
            type="button"
            variant="outline"
            className="resource-workspace-accent-button"
            onClick={() => onToast("New variable is coming soon")}
          >
            <PlusIcon data-icon="inline-start" />
            New Variable
          </Button>
        </div>
      </div>
      <div className="resource-workspace-hint-banner">
        <p>
          Connect this resource to an application with a Variable Reference.
        </p>
        <button
          className="resource-workspace-close-hint"
          type="button"
          aria-label="Dismiss variables hint"
          onClick={(event) => event.currentTarget.parentElement?.remove()}
        >
          <XIcon aria-hidden="true" />
        </button>
      </div>
      <ul className="resource-workspace-variable-list">
        {filteredVariables.length ? (
          filteredVariables.map(([name, value]) => (
            <li key={name} className="resource-workspace-variable-row">
              <code>{name}</code>
              <span>{value}</span>
              <button
                type="button"
                aria-label={`Copy ${name}`}
                onClick={() => onToast(`${name} copied`)}
              >
                <CopyIcon aria-hidden="true" />
              </button>
            </li>
          ))
        ) : (
          <li className="resource-workspace-variable-empty">
            No variables match that search.
          </li>
        )}
      </ul>
    </section>
  )
}

function MetricsPane({
  node,
  projectSlug,
  workspaceSlug,
  onToast,
}: {
  node: ResourceWorkspaceNode
  projectSlug: string
  workspaceSlug: string
  onToast: (message: string) => void
}) {
  const resourceId = node.resource?.id
  const isDatabase = node.resource?.resourceType === "postgres"
  const isAppService = node.resource?.resourceType === "app"
  const isSupportedResource = isDatabase || isAppService
  const isReady = isSupportedResource && node.resource?.status === "ready"
  const isError = node.resource?.status === "error"
  const resourceLabel = isAppService ? "app service" : "project database"
  const [metrics, setMetrics] = useState<ResourceMetrics | null>(null)
  const [loading, setLoading] = useState(true)
  const [refreshing, setRefreshing] = useState(false)
  const [live, setLive] = useState(true)
  const [range, setRange] = useState<DatabaseMetricsRange>("24h")
  const [error, setError] = useState<string | null>(null)
  const requestInFlight = useRef<string | null>(null)
  const requestSequence = useRef(0)

  const loadMetrics = useCallback(
    async (silent: boolean) => {
      if (!resourceId || !isReady) {
        return
      }
      const requestKey = `${resourceId}:${range}`
      if (requestInFlight.current === requestKey) {
        return
      }
      requestInFlight.current = requestKey
      const sequence = requestSequence.current + 1
      requestSequence.current = sequence
      if (silent) {
        setRefreshing(true)
      } else {
        setLoading(true)
      }
      try {
        const nextMetrics = isAppService
          ? await getAppServiceMetrics(
              workspaceSlug,
              projectSlug,
              resourceId,
              range
            )
          : await getDatabaseMetrics(
              workspaceSlug,
              projectSlug,
              resourceId,
              range
            )
        if (sequence === requestSequence.current) {
          setMetrics(nextMetrics)
          setError(null)
        }
      } catch (requestError) {
        if (sequence === requestSequence.current) {
          setError(databaseErrorMessage(requestError))
        }
      } finally {
        if (requestInFlight.current === requestKey) {
          requestInFlight.current = null
        }
        if (sequence === requestSequence.current) {
          setLoading(false)
          setRefreshing(false)
        }
      }
    },
    [isAppService, isReady, projectSlug, range, resourceId, workspaceSlug]
  )

  useEffect(() => {
    if (!isReady || !resourceId) {
      return
    }

    const initialLoadId = window.setTimeout(() => {
      void loadMetrics(false)
    }, 0)
    if (!live) {
      return () => window.clearTimeout(initialLoadId)
    }
    const intervalId = window.setInterval(() => {
      void loadMetrics(true)
    }, METRICS_REFRESH_MS)
    return () => {
      window.clearTimeout(initialLoadId)
      window.clearInterval(intervalId)
    }
  }, [isReady, live, loadMetrics, resourceId])

  if (!isSupportedResource || !resourceId) {
    return (
      <section
        className="resource-workspace-pane"
        id="resource-pane-metrics"
        role="tabpanel"
        aria-labelledby="resource-tab-metrics"
      >
        <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title="No resource metrics"
          description="Metrics are available for a ready PostgreSQL or Docker app service resource."
        />
      </section>
    )
  }

  if (!isReady) {
    return (
      <section
        className="resource-workspace-pane"
        id="resource-pane-metrics"
        role="tabpanel"
        aria-labelledby="resource-tab-metrics"
      >
        <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title={
            isError
              ? `${isAppService ? "App service" : "Database"} is unavailable`
              : `${isAppService ? "App service" : "Database"} is provisioning`
          }
          description={
            isError
              ? node.resource?.errorMessage ??
                `Live metrics are unavailable for this ${resourceLabel}.`
              : `Live metrics become available as soon as the ${resourceLabel} is ready.`
          }
        />
      </section>
    )
  }

  const points = metrics?.points ?? []
  const current = points[points.length - 1] ?? null
  const selectedRangeLabel =
    METRIC_RANGE_OPTIONS.find((option) => option.value === range)?.label ??
    "Last 24 hours"
  const lastSampleTimestamp = current?.timestamp ?? metrics?.toTimestamp
  const lastUpdated = lastSampleTimestamp
    ? new Date(lastSampleTimestamp * 1_000).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : null
  const retentionDays = Math.round(
    (metrics?.retentionSeconds ?? 30 * 24 * 60 * 60) / (24 * 60 * 60)
  )
  const sampleInterval = metrics?.sampleIntervalSeconds ?? 5
  const resolution = metrics?.resolutionSeconds ?? sampleInterval
  const telemetryLabel =
    loading && !metrics
      ? "Connecting telemetry"
      : metrics?.systemMetricsAvailable === false
        ? "Limited telemetry"
        : "Telemetry active"
  const metricCards: MetricCardProps[] = [
    {
      title: "CPU",
      description: "Container usage",
      legend: "CPU utilization",
      accent: "blue",
      featured: true,
      points,
      rangeLabel: selectedRangeLabel,
      series: [
        {
          label: "CPU",
          colorClass: "blue",
          getValue: (point) => point.cpuPercent,
          formatValue: (value) => `${value.toFixed(2)}%`,
        },
      ],
      scaleMax: 100,
      axisMaxLabel: "100%",
    },
    {
      title: "Memory",
      description: "Container usage",
      legend: "Memory utilization",
      accent: "violet",
      featured: true,
      points,
      rangeLabel: selectedRangeLabel,
      series: [
        {
          label: "Used",
          colorClass: "violet",
          getValue: (point) => memoryPercent(point),
          formatValue: (value) => `${value.toFixed(1)}%`,
          getDisplayValue: (point) =>
            point.memoryUsedBytes !== null && point.memoryLimitBytes !== null
              ? `${formatBytes(point.memoryUsedBytes)} / ${formatBytes(point.memoryLimitBytes)}`
              : "Unavailable",
        },
      ],
      scaleMax: 100,
      axisMaxLabel: "100%",
    },
    {
      title: "Volume",
      description: "Persistent storage usage",
      legend: "Used · capacity",
      accent: "ink",
      points,
      rangeLabel: selectedRangeLabel,
      series: [
        {
          label: "Used",
          colorClass: "ink",
          getValue: (point) => volumePercent(point),
          formatValue: (value) => `${value.toFixed(1)}%`,
          getDisplayValue: (point) =>
            point.volumeUsedBytes !== null && point.volumeCapacityBytes !== null
              ? `${formatBytes(point.volumeUsedBytes)} / ${formatBytes(point.volumeCapacityBytes)}`
              : point.volumeUsedBytes !== null
                ? formatBytes(point.volumeUsedBytes)
                : "Unavailable",
        },
      ],
      scaleMax: 100,
      axisMaxLabel: "100%",
    },
    {
      title: "Network I/O",
      description: "Container network counters",
      legend: "RX · TX totals",
      accent: "green",
      points,
      rangeLabel: selectedRangeLabel,
      series: [
        {
          label: "RX",
          colorClass: "green",
          getValue: (point) => point.networkReceiveBytes,
          formatValue: formatBytes,
        },
        {
          label: "TX",
          colorClass: "blue",
          getValue: (point) => point.networkTransmitBytes,
          formatValue: formatBytes,
        },
      ],
    },
  ]
  if (isAppService) {
    metricCards.push(
      {
        title: "Public Network Traffic",
        description: "Traffic to and from the internet",
        legend: "Inbound · outbound payload",
        accent: "green",
        points,
        rangeLabel: selectedRangeLabel,
        series: [
          {
            label: "Inbound",
            colorClass: "green",
            getValue: (point) => point.publicNetworkReceiveBytes ?? null,
            formatValue: formatBytes,
          },
          {
            label: "Outbound",
            colorClass: "blue",
            getValue: (point) => point.publicNetworkTransmitBytes ?? null,
            formatValue: formatBytes,
          },
        ],
      },
      {
        title: "Requests",
        description: "Public HTTP requests",
        legend: "Request count",
        accent: "blue",
        points,
        rangeLabel: selectedRangeLabel,
        axisFormat: formatCount,
        series: [
          {
            label: "Requests",
            colorClass: "blue",
            getValue: (point) => point.requests ?? null,
            formatValue: formatCount,
          },
        ],
      },
      {
        title: "Response Time",
        description: "Average request latency",
        legend: "Average latency",
        accent: "orange",
        points,
        rangeLabel: selectedRangeLabel,
        axisFormat: formatMilliseconds,
        series: [
          {
            label: "Average",
            colorClass: "orange",
            getValue: (point) => point.responseTimeMs ?? null,
            formatValue: (value) => `${value.toFixed(0)} ms`,
          },
        ],
      },
      {
        title: "Request Error Rate",
        description: "4xx and 5xx responses",
        legend: "Failed request rate",
        accent: "violet",
        points,
        rangeLabel: selectedRangeLabel,
        scaleMax: 100,
        axisMaxLabel: "100%",
        series: [
          {
            label: "Errors",
            colorClass: "violet",
            getValue: (point) => point.requestErrorRate ?? null,
            formatValue: (value) => `${value.toFixed(2)}%`,
          },
        ],
      }
    )
  }
  metricCards.push({
    title: "Disk I/O",
    description: "Container storage counters",
    legend: "Read · write totals",
    accent: "orange",
    points,
    rangeLabel: selectedRangeLabel,
    series: [
      {
        label: "Read",
        colorClass: "orange",
        getValue: (point) => point.diskReadBytes,
        formatValue: formatBytes,
      },
      {
        label: "Write",
        colorClass: "ink",
        getValue: (point) => point.diskWriteBytes,
        formatValue: formatBytes,
      },
    ],
  })

  return (
    <section
      className="resource-workspace-pane resource-workspace-metrics-pane"
      id="resource-pane-metrics"
      role="tabpanel"
      aria-labelledby="resource-tab-metrics"
    >
      <div className="resource-workspace-metrics-header">
        <div className="resource-workspace-metrics-heading">
          <span className="resource-workspace-metrics-eyebrow">
            <ActivityIcon aria-hidden="true" /> Observability
          </span>
          <h3>Runtime metrics</h3>
          <p>
            {node.title} · {resourceLabel}
          </p>
        </div>
        <div className="resource-workspace-metrics-controls">
          <label className="resource-workspace-metric-range">
            <span>Range</span>
            <select
              aria-label="Metric time range"
              value={range}
              onChange={(event) => {
                const nextRange = event.target.value as DatabaseMetricsRange
                setRange(nextRange)
                setMetrics(null)
                setError(null)
                setLoading(true)
              }}
            >
              {METRIC_RANGE_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            className="resource-workspace-metrics-control"
            aria-pressed={live}
            onClick={() => {
              const nextLive = !live
              setLive(nextLive)
              onToast(nextLive ? "Live metrics resumed" : "Live metrics paused")
            }}
          >
            <span
              className={cn("resource-workspace-live-dot", !live && "paused")}
              aria-hidden="true"
            />
            {live ? "Live" : "Paused"}
          </button>
          <button
            type="button"
            className="resource-workspace-metrics-control"
            aria-label="Refresh metrics"
            disabled={loading || refreshing}
            onClick={() => void loadMetrics(true)}
          >
            <RefreshCwIcon
              aria-hidden="true"
              className={cn(refreshing && "resource-workspace-refreshing-icon")}
            />
            <span className="resource-workspace-refresh-label">Refresh</span>
          </button>
        </div>
      </div>

      <div className="resource-workspace-metrics-meta">
        <div className="resource-workspace-metrics-telemetry">
          <span
            className={cn(
              "resource-workspace-metrics-pulse",
              metrics?.systemMetricsAvailable === false && "limited",
              loading && !metrics && "loading",
              !live && "paused"
            )}
            aria-hidden="true"
          >
            <span />
          </span>
          <span>
            <strong>{telemetryLabel}</strong>
            <small>
              {refreshing
                ? "Syncing latest sample…"
                : points.length
                  ? `${points.length} samples · every ${formatMetricDuration(sampleInterval)}`
                  : "Waiting for the first sample"}
            </small>
          </span>
        </div>
        <dl className="resource-workspace-metrics-meta-list">
          <div>
            <dt>Last sample</dt>
            <dd>{lastUpdated ?? "—"}</dd>
          </div>
          <div>
            <dt>Resolution</dt>
            <dd>{formatMetricDuration(resolution)}</dd>
          </div>
          <div>
            <dt>Retention</dt>
            <dd>{retentionDays} days</dd>
          </div>
          <div>
            <dt>Provider</dt>
            <dd>{metrics?.provider ?? "—"}</dd>
          </div>
          <div>
            <dt>Window</dt>
            <dd>
              {metrics?.fromTimestamp !== undefined
                ? `${formatMetricAxisTimestamp(metrics.fromTimestamp)} – ${formatMetricAxisTimestamp(metrics.toTimestamp)}`
                : "—"}
            </dd>
          </div>
        </dl>
      </div>

      {error && (
        <div className="resource-workspace-error" role="alert">
          {error}
        </div>
      )}
      {metrics?.systemMetricsMessage && (
        <div className="resource-workspace-hint-banner">
          <p>{metrics.systemMetricsMessage}</p>
        </div>
      )}

      {loading && !metrics ? (
        <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title="Loading metrics…"
          description={`Reading live runtime metrics from this ${resourceLabel}.`}
        />
      ) : (
        <div className="resource-workspace-metrics-grid">
          {metricCards.map((metric) => (
            <MetricCard key={metric.title} {...metric} />
          ))}
        </div>
      )}
    </section>
  )
}

type MetricAccent = "blue" | "violet" | "ink" | "green" | "orange"

type MetricSeries = {
  label: string
  colorClass: MetricAccent
  getValue: (point: ResourceMetricPoint) => number | null
  formatValue: (value: number) => string
  getDisplayValue?: (point: ResourceMetricPoint) => string
}

type MetricCardProps = {
  title: string
  description: string
  legend: string
  accent: MetricAccent
  featured?: boolean
  points: ResourceMetricPoint[]
  series: MetricSeries[]
  rangeLabel: string
  scaleMax?: number
  axisMaxLabel?: string
  axisFormat?: (value: number) => string
}

function MetricCard({
  title,
  description,
  legend,
  accent,
  featured = false,
  points,
  series,
  rangeLabel,
  scaleMax,
  axisMaxLabel,
  axisFormat,
}: MetricCardProps) {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null)
  const current = points[points.length - 1] ?? null
  const allValues = series.flatMap((item) =>
    points.flatMap((point) => {
      const value = item.getValue(point)
      return value === null || !Number.isFinite(value) ? [] : [value]
    })
  )
  const measuredMax = allValues.length ? Math.max(...allValues) : 0
  const chartMax = scaleMax ?? Math.max(measuredMax, 1)
  const chartWidth = 310
  const chartLeft = 40
  const chartBottom = 156
  const chartTop = 18
  const chartHeight = chartBottom - chartTop
  const xForIndex = (index: number) =>
    chartLeft +
    (points.length > 1
      ? (index * chartWidth) / (points.length - 1)
      : chartWidth / 2)
  const yForValue = (value: number) =>
    chartBottom -
    (Math.max(0, Math.min(value, chartMax)) / chartMax) * chartHeight
  const hasChartData = allValues.length > 0
  const axisLabel = !hasChartData
    ? "—"
    : (axisMaxLabel ??
      (measuredMax === 0
        ? axisFormat?.(0) ?? formatBytes(0)
        : axisFormat?.(chartMax) ?? formatBytes(chartMax)))
  const axisMidLabel = !hasChartData
    ? "—"
    : measuredMax === 0
      ? axisFormat?.(0) ?? formatBytes(0)
      : axisFormat
        ? axisFormat(chartMax / 2)
        : axisMaxLabel?.endsWith("%")
          ? `${Math.round(chartMax / 2)}%`
          : formatBytes(chartMax / 2)
  const hoveredPoint =
    hoveredIndex === null ? null : (points[hoveredIndex] ?? null)
  const hoveredX = hoveredIndex === null ? null : xForIndex(hoveredIndex)
  const cardClassName = cn(
    "resource-workspace-metric-card",
    accent,
    featured && "featured",
    !hasChartData && "is-empty"
  )
  const tooltipId = `metric-tooltip-${title.toLowerCase().replaceAll(" ", "-")}`
  const tickIndices = getMetricTickIndices(points.length)

  function handleChartMove(event: React.MouseEvent<SVGSVGElement>) {
    if (!points.length) {
      return
    }
    const rect = event.currentTarget.getBoundingClientRect()
    if (!rect.width) {
      setHoveredIndex(0)
      return
    }
    const viewBoxX = ((event.clientX - rect.left) / rect.width) * 360
    const progress = Math.max(
      0,
      Math.min(1, (viewBoxX - chartLeft) / chartWidth)
    )
    const index =
      points.length === 1 ? 0 : Math.round(progress * (points.length - 1))
    setHoveredIndex(index)
  }

  function handleChartKeyDown(event: React.KeyboardEvent<SVGSVGElement>) {
    if (!points.length) return
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      event.preventDefault()
      const direction = event.key === "ArrowLeft" ? -1 : 1
      setHoveredIndex((index) => {
        const nextIndex = (index ?? points.length - 1) + direction
        return Math.max(0, Math.min(points.length - 1, nextIndex))
      })
    }
    if (event.key === "Escape") {
      setHoveredIndex(null)
    }
  }

  return (
    <article className={cardClassName}>
      <div className="resource-workspace-metric-card-head">
        <div>
          <span className="resource-workspace-metric-label">Metric</span>
          <h3>{title}</h3>
          <p>{description}</p>
        </div>
        <span className="resource-workspace-metric-legend">
          <i className={accent} aria-hidden="true" /> {legend}
        </span>
      </div>
      <div
        className="resource-workspace-metric-values"
        aria-label={`${title} current values`}
      >
        {series.map((item) => {
          return (
            <span key={item.label} className={item.colorClass}>
              <small>{item.label}</small>
              <strong>
                {current
                  ? metricSeriesDisplayValue(item, current)
                  : "Waiting for sample…"}
              </strong>
            </span>
          )
        })}
      </div>
      <div className="resource-workspace-chart-wrap">
        <svg
          viewBox="0 0 360 190"
          preserveAspectRatio="none"
          role="img"
          aria-label={`${title} usage, ${rangeLabel.toLowerCase()}`}
          tabIndex={points.length ? 0 : -1}
          aria-describedby={hoveredPoint ? tooltipId : undefined}
          onMouseMove={handleChartMove}
          onMouseLeave={() => setHoveredIndex(null)}
          onFocus={() =>
            setHoveredIndex(points.length ? points.length - 1 : null)
          }
          onKeyDown={handleChartKeyDown}
        >
          <g className="resource-workspace-chart-grid">
            <line x1="40" y1="18" x2="350" y2="18" />
            <line x1="40" y1="52.5" x2="350" y2="52.5" />
            <line x1="40" y1="87" x2="350" y2="87" />
            <line x1="40" y1="121.5" x2="350" y2="121.5" />
            <line x1="40" y1="156" x2="350" y2="156" />
            <line x1="40" y1="18" x2="40" y2="156" />
            <line x1="350" y1="18" x2="350" y2="156" />
          </g>
          <text x="0" y="22">
            {axisLabel}
          </text>
          <text x="0" y="90">
            {axisMidLabel}
          </text>
          <text x="0" y="160">
            0
          </text>
          {tickIndices.map((index, tickIndex) => (
            <text
              key={index}
              className="resource-workspace-chart-axis-label"
              x={xForIndex(index)}
              y="178"
              textAnchor={
                tickIndex === 0
                  ? "start"
                  : tickIndex === tickIndices.length - 1
                    ? "end"
                    : "middle"
              }
            >
              {formatMetricAxisTimestamp(points[index]?.timestamp)}
            </text>
          ))}
          {hoveredPoint && hoveredX !== null && (
            <line
              className="resource-workspace-chart-hover-line"
              x1={hoveredX}
              y1={chartTop}
              x2={hoveredX}
              y2={chartBottom}
            />
          )}
          {points.length > 0 &&
            series.map((item) => {
              const linePoints = points
                .map((point, index) => {
                  const value = item.getValue(point)
                  if (value === null || !Number.isFinite(value)) {
                    return null
                  }
                  return `${xForIndex(index)},${yForValue(value)}`
                })
                .filter((point): point is string => point !== null)
                .join(" ")
              return linePoints ? (
                <g key={item.label}>
                  <polygon
                    className={cn(
                      "resource-workspace-chart-area",
                      item.colorClass
                    )}
                    points={`${linePoints} ${xForIndex(points.length - 1)},${chartBottom} ${xForIndex(0)},${chartBottom}`}
                  />
                  <polyline
                    className={cn(
                      "resource-workspace-chart-line",
                      item.colorClass
                    )}
                    points={linePoints}
                  />
                </g>
              ) : null
            })}
          {hoveredPoint &&
            hoveredX !== null &&
            series.map((item) => {
              const value = item.getValue(hoveredPoint)
              return value === null || !Number.isFinite(value) ? null : (
                <circle
                  key={item.label}
                  className={cn(
                    "resource-workspace-chart-hover-point",
                    item.colorClass
                  )}
                  cx={hoveredX}
                  cy={yForValue(value)}
                  r="4.5"
                />
              )
            })}
        </svg>
        {!hasChartData && (
          <div className="resource-workspace-chart-empty" role="note">
            <span aria-hidden="true">—</span>
            <strong>No samples for this series</strong>
            <small>Telemetry is not reported by this resource.</small>
          </div>
        )}
        {hoveredPoint && hoveredX !== null && (
          <div
            className="resource-workspace-chart-tooltip"
            role="status"
            id={tooltipId}
            style={{
              left: `${Math.min(92, Math.max(8, (hoveredX / 360) * 100))}%`,
            }}
          >
            <time>{formatMetricTimestamp(hoveredPoint.timestamp)}</time>
            {series.map((item) => (
              <span key={item.label}>
                <i className={item.colorClass} aria-hidden="true" />
                <small>{item.label}</small>
                <strong>{metricSeriesDisplayValue(item, hoveredPoint)}</strong>
              </span>
            ))}
          </div>
        )}
      </div>
    </article>
  )
}

function metricSeriesDisplayValue(
  series: MetricSeries,
  point: ResourceMetricPoint
) {
  const value = series.getValue(point)
  return (
    series.getDisplayValue?.(point) ??
    (value === null ? "Unavailable" : series.formatValue(value))
  )
}

function formatMetricTimestamp(timestamp: number) {
  return new Date(timestamp * 1_000).toLocaleString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  })
}

function formatMetricAxisTimestamp(timestamp: number | undefined) {
  if (timestamp === undefined) {
    return "—"
  }
  return new Date(timestamp * 1_000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  })
}

function getMetricTickIndices(pointCount: number) {
  if (pointCount <= 0) {
    return []
  }
  if (pointCount === 1) {
    return [0]
  }
  const middle = Math.floor((pointCount - 1) / 2)
  return middle === 0 || middle === pointCount - 1
    ? [0, pointCount - 1]
    : [0, middle, pointCount - 1]
}

function formatMetricDuration(seconds: number) {
  if (!Number.isFinite(seconds) || seconds <= 0) {
    return "—"
  }
  if (seconds < 60) {
    return `${Math.round(seconds)}s`
  }
  if (seconds < 60 * 60) {
    return `${Math.round(seconds / 60)}m`
  }
  if (seconds < 24 * 60 * 60) {
    return `${Math.round(seconds / (60 * 60))}h`
  }
  return `${Math.round(seconds / (24 * 60 * 60))}d`
}

function formatCount(value: number) {
  return new Intl.NumberFormat(undefined, {
    maximumFractionDigits: 0,
  }).format(Math.max(0, value))
}

function formatMilliseconds(value: number) {
  return `${Math.round(Math.max(0, value))} ms`
}

function memoryPercent(point: ResourceMetricPoint) {
  if (
    point.memoryUsedBytes === null ||
    point.memoryLimitBytes === null ||
    point.memoryLimitBytes <= 0
  ) {
    return null
  }
  return Math.min(100, (point.memoryUsedBytes / point.memoryLimitBytes) * 100)
}

function volumePercent(point: ResourceMetricPoint) {
  if (
    point.volumeUsedBytes === null ||
    point.volumeCapacityBytes === null ||
    point.volumeCapacityBytes <= 0
  ) {
    return null
  }
  return Math.min(
    100,
    (point.volumeUsedBytes / point.volumeCapacityBytes) * 100
  )
}

function ConsolePane({
  node,
  workspaceSlug,
  projectSlug,
  onToast,
}: {
  node: ResourceWorkspaceNode
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
}) {
  const appService =
    node.resource?.resourceType === "app" ? node.resource : null
  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-console"
      role="tabpanel"
      aria-labelledby="resource-tab-console"
    >
      <div className="resource-workspace-console">
        <div className="resource-workspace-console-meta">
          <span>
            <TerminalIcon aria-hidden="true" />
            {appService ? "Container logs" : "Project console"}
          </span>
          <span className="resource-workspace-muted">
            {appService ? "Docker runtime" : "Not connected"}
          </span>
        </div>
        {appService ? (
          <AppServiceRuntimeLogs
            appService={appService}
            workspaceSlug={workspaceSlug}
            projectSlug={projectSlug}
          />
        ) : (
          <>
            <ResourceEmptyState
              icon={<ServerIcon aria-hidden="true" />}
              title="Console is unavailable"
              description="This managed PostgreSQL resource exposes connection credentials, not a shell."
            />
            <Button
              type="button"
              variant="outline"
              onClick={() => onToast("Connection details opened")}
            >
              <CopyIcon data-icon="inline-start" />
              View connection details
            </Button>
          </>
        )}
      </div>
    </section>
  )
}

function AutoDeployEditor({
  appService,
  workspaceSlug,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const isGithubImage = appService.imageSource === "github"
  const [enabled, setEnabled] = useState(appService.autoDeployEnabled ?? false)
  const [isSaving, setIsSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const isBusy = isSaving || appService.status === "provisioning"

  async function handleChange(event: React.ChangeEvent<HTMLInputElement>) {
    const nextEnabled = event.target.checked
    setEnabled(nextEnabled)
    setIsSaving(true)
    setError(null)
    try {
      const resource = await updateAppServiceAutoDeploy(
        workspaceSlug,
        projectSlug,
        appService.id,
        { enabled: nextEnabled }
      )
      setEnabled(resource.autoDeployEnabled ?? nextEnabled)
      onAppServiceUpdated?.(resource)
      onToast(
        nextEnabled
          ? "Automatic GitHub image deploys enabled."
          : "Automatic GitHub image deploys disabled."
      )
    } catch (caught) {
      setEnabled(appService.autoDeployEnabled ?? false)
      setError(
        caught instanceof ApiError
          ? caught.message
          : "Automatic image deploys could not be updated."
      )
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <div className="resource-workspace-auto-deploy">
      <div className="resource-workspace-auto-deploy-row">
        <div>
          <strong>Deploy new image digests</strong>
          <p className="resource-workspace-muted">
            {isGithubImage
              ? "Knotree checks this GHCR tag every minute and redeploys only when the image changes."
              : "Automatic image deploys are available for GitHub Container Registry images."}
          </p>
        </div>
        <label className="resource-workspace-auto-deploy-toggle">
          <input
            type="checkbox"
            aria-label="Auto deploy new GitHub images"
            checked={enabled}
            disabled={!isGithubImage || isBusy}
            onChange={(event) => void handleChange(event)}
          />
          <span>{isSaving ? "Saving…" : enabled ? "Enabled" : "Disabled"}</span>
        </label>
      </div>
      <dl>
        <div>
          <dt>Registry</dt>
          <dd>{isGithubImage ? "GitHub Container Registry" : "Docker registry"}</dd>
        </div>
        <div>
          <dt>Deployed digest</dt>
          <dd>
            <code>{appService.deployedImageDigest ?? "Not recorded yet"}</code>
          </dd>
        </div>
        <div>
          <dt>Last check</dt>
          <dd>
            {appService.autoDeployCheckedAt
              ? new Date(appService.autoDeployCheckedAt).toLocaleString()
              : "Not checked yet"}
          </dd>
        </div>
      </dl>
      {appService.autoDeployError ? (
        <p className="resource-workspace-app-port-error" role="alert">
          {appService.autoDeployError}
        </p>
      ) : null}
      {error ? (
        <p className="resource-workspace-app-port-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  )
}

function AppPortEditor({
  appService,
  workspaceSlug,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const [appPort, setAppPort] = useState(String(appService.appPort))
  const [error, setError] = useState<string | null>(null)
  const [isSaving, setIsSaving] = useState(false)
  const parsedPort = Number(appPort)
  const isValidPort =
    Number.isInteger(parsedPort) && parsedPort >= 1 && parsedPort <= 65535
  const isUnchanged = isValidPort && parsedPort === appService.appPort
  const isBusy = appService.status === "provisioning" || isSaving

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!isValidPort) {
      setError("Use a container port between 1 and 65535.")
      return
    }
    if (isUnchanged || isBusy) {
      return
    }
    setIsSaving(true)
    setError(null)
    try {
      const resource = await updateAppService(
        workspaceSlug,
        projectSlug,
        appService.id,
        { appPort: parsedPort }
      )
      onAppServiceUpdated?.(resource)
      onToast("Container port updated. The app was redeployed.")
    } catch (caught) {
      if (caught instanceof ApiError) {
        setError(caught.fields.appPort ?? caught.message)
      } else {
        setError("The API is currently unavailable. Please try again.")
      }
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <form className="resource-workspace-app-port" onSubmit={handleSubmit}>
      <div className="resource-workspace-app-port-field">
        <Label htmlFor="workspace-app-port">Container port</Label>
        <Input
          id="workspace-app-port"
          name="appPort"
          type="number"
          min={1}
          max={65535}
          value={appPort}
          disabled={isBusy}
          aria-invalid={Boolean(error)}
          aria-describedby="workspace-app-port-hint workspace-app-port-error"
          onChange={(event) => {
            setAppPort(event.target.value)
            setError(null)
          }}
        />
      </div>
      <p id="workspace-app-port-hint" className="resource-workspace-muted">
        The port the process listens on inside the container. Saving redeploys
        the app and assigns a new public URL.
      </p>
      {error ? (
        <p
          id="workspace-app-port-error"
          className="resource-workspace-app-port-error"
          role="alert"
        >
          {error}
        </p>
      ) : null}
      <Button type="submit" size="sm" disabled={isBusy || isUnchanged}>
        {isSaving ? "Redeploying…" : "Save and redeploy"}
      </Button>
    </form>
  )
}

function DatabaseAttachmentEditor({
  appService,
  workspaceSlug,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const [resources, setResources] = useState<PostgresResource[]>([])
  const [selectedResourceId, setSelectedResourceId] = useState(
    appService.databaseConnection?.resourceId ?? ""
  )
  const [isLoading, setIsLoading] = useState(true)
  const [isSaving, setIsSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    void listPostgresResources(workspaceSlug, projectSlug)
      .then((nextResources) => {
        if (active) {
          setResources(nextResources)
          setError(null)
        }
      })
      .catch((caught: unknown) => {
        if (!active) {
          return
        }
        setError(
          caught instanceof ApiError
            ? caught.message
            : "The database resources could not be loaded."
        )
      })
      .finally(() => {
        if (active) {
          setIsLoading(false)
        }
      })

    return () => {
      active = false
    }
  }, [projectSlug, workspaceSlug])

  async function handleChange(event: React.ChangeEvent<HTMLSelectElement>) {
    const nextResourceId = event.target.value
    const nextDatabaseResourceId = nextResourceId || null
    setSelectedResourceId(nextResourceId)
    setIsSaving(true)
    setError(null)
    try {
      const resource = await updateAppServiceDatabase(
        workspaceSlug,
        projectSlug,
        appService.id,
        { databaseResourceId: nextDatabaseResourceId }
      )
      onAppServiceUpdated?.(resource)
      onToast(
        nextDatabaseResourceId
          ? "Postgres connection assigned. The service was redeployed."
          : "Postgres connection removed. The service was redeployed."
      )
    } catch (caught) {
      setSelectedResourceId(appService.databaseConnection?.resourceId ?? "")
      setError(
        caught instanceof ApiError
          ? caught.message
          : "The database connection could not be updated."
      )
    } finally {
      setIsSaving(false)
    }
  }

  const isBusy = isLoading || isSaving || appService.status === "provisioning"

  return (
    <article
      id="database-connection"
      className="resource-workspace-setting-section"
    >
      <h3>Database connection</h3>
      <p>
        A new app service starts without a database. Choose the PostgreSQL
        resource to inject private connection variables into this service.
      </p>
      <div className="resource-workspace-setting-block">
        <label className="resource-workspace-attachment-field">
          <span>PostgreSQL resource</span>
          <select
            aria-label="PostgreSQL resource for app service"
            value={selectedResourceId}
            disabled={isBusy}
            onChange={(event) => void handleChange(event)}
          >
            <option value="">No database attached</option>
            {resources.map((resource) => (
              <option
                key={resource.id}
                value={resource.id}
                disabled={resource.status !== "ready"}
              >
                {resource.name} ({resource.status})
              </option>
            ))}
          </select>
        </label>
        {isSaving && (
          <span className="resource-workspace-muted">Redeploying service…</span>
        )}
      </div>
      {error ? (
        <p className="resource-workspace-app-port-error" role="alert">
          {error}
        </p>
      ) : null}
    </article>
  )
}

function PublicAccessEditor({
  appService,
  workspaceSlug,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const [enabled, setEnabled] = useState(Boolean(appService.publicAccessEnabled))
  const [rateLimitRpm, setRateLimitRpm] = useState(
    String(appService.rateLimitRpm ?? 60)
  )
  const [isSaving, setIsSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function handleSave() {
    const parsed = Number.parseInt(rateLimitRpm, 10)
    if (!Number.isFinite(parsed) || parsed < 1 || parsed > 10_000) {
      setError("Rate limit must be between 1 and 10000 requests per minute.")
      return
    }
    setIsSaving(true)
    setError(null)
    try {
      const resource = await updateAppServicePublicAccess(
        workspaceSlug,
        projectSlug,
        appService.id,
        { enabled, rateLimitRpm: parsed }
      )
      onAppServiceUpdated?.(resource)
      onToast(
        resource.publicAccessEnabled
          ? `Public hostname ${resource.publicDomain ?? "assigned"}`
          : "Public access disabled"
      )
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : "Public access could not be updated."
      )
    } finally {
      setIsSaving(false)
    }
  }

  return (
    <article
      id="public-access"
      className="resource-workspace-setting-section"
    >
      <h3>Public access</h3>
      <p>
        A random *.knotree.org hostname is assigned only after public access is
        enabled. Kong enforces the custom rate limit for that hostname.
      </p>
      <div className="resource-workspace-setting-block">
        <label className="resource-workspace-attachment-field">
          <span>Expose on *.knotree.org</span>
          <input
            type="checkbox"
            aria-label="Enable public hostname"
            checked={enabled}
            disabled={isSaving}
            onChange={(event) => setEnabled(event.target.checked)}
          />
        </label>
        <label className="resource-workspace-attachment-field">
          <span>Rate limit (req/min)</span>
          <input
            aria-label="App service rate limit"
            type="number"
            min={1}
            max={10000}
            value={rateLimitRpm}
            disabled={isSaving}
            onChange={(event) => setRateLimitRpm(event.target.value)}
          />
        </label>
        <Button
          type="button"
          size="sm"
          disabled={isSaving}
          onClick={() => void handleSave()}
        >
          Save public access
        </Button>
      </div>
      {error ? (
        <p className="resource-workspace-app-port-error" role="alert">
          {error}
        </p>
      ) : null}
    </article>
  )
}

function SettingsPane({
  node,
  environment,
  workspaceSlug,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  node: ResourceWorkspaceNode
  environment: string
  workspaceSlug: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const [search, setSearch] = useState("")
  const appService =
    node.resource?.resourceType === "app" ? node.resource : undefined
  const sections = [
    {
      id: "source",
      title: "Source",
      description: "The resource provisioned for this project environment.",
      rows: [
        ["Resource", node.type],
        ["Status", node.status],
      ],
    },
    {
      id: "limits",
      title: "Resource limits",
      description:
        "Hard limits applied to this resource so it cannot consume the host beyond its allocation.",
      rows: [
        ["CPU", "1 vCPU"],
        ["Memory", "1 GB RAM"],
        ["Volume", "10 GB (writes stop at the limit)"],
      ],
    },
    ...(appService
      ? [
          {
            id: "auto-deploy",
            title: "Auto updates",
            description:
              "Watch the GitHub Container Registry tag and queue a deployment when its image digest changes.",
            rows: [
              ["Image", appService.image],
              [
                "Deployed digest",
                appService.deployedImageDigest ?? "Not recorded yet",
              ],
            ],
          },
        ]
      : []),
    {
      id: "networking",
      title: "Networking",
      description: appService
        ? "Image endpoint, container port, and the public URL Knotree assigned."
        : "Connection endpoint and access scope for this resource.",
      rows:
        node.resource?.resourceType === "postgres"
          ? [
              ["Host", node.resource.host],
              ["Port", String(node.resource.port)],
              ["Database", node.resource.databaseName],
              ["Username", node.resource.username],
            ]
          : appService
            ? [
                ["Image", appService.image],
                [
                  "Public access",
                  appService.publicAccessEnabled ? "Enabled" : "Disabled",
                ],
                ["Public domain", appService.publicDomain ?? "Not assigned"],
                ["Public URL", appService.serviceUrl ?? "Private"],
                [
                  "Rate limit",
                  `${appService.rateLimitRpm ?? 60} req/min`,
                ],
                ["Container", appService.containerName ?? "Pending"],
              ]
            : node.resource?.resourceType === "redis"
              ? [
                  ["Host", node.resource.host],
                  ["Port", String(node.resource.port)],
                  ["Network alias", node.resource.networkAlias],
                ]
              : [["Access", "Project internal"]],
    },
    {
      id: "service",
      title: "Service",
      description: "Identity used by the topology node and environment.",
      rows: [
        ["Name", node.title],
        ["Type", node.type],
        ["Volume", node.volume],
        ["Environment", environment],
      ],
    },
  ]
  const visibleSections = sections.filter((section) => {
    const haystack = `${section.title} ${section.description} ${section.rows.flat().join(" ")}${
      section.id === "networking" && appService ? " container port" : ""
    }`
    return haystack.toLowerCase().includes(search.toLowerCase())
  })

  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-settings"
      role="tabpanel"
      aria-labelledby="resource-tab-settings"
    >
      <div className="resource-workspace-settings-filter">
        <SearchIcon aria-hidden="true" />
        <Input
          aria-label="Filter settings"
          value={search}
          placeholder="Filter settings..."
          onChange={(event) => setSearch(event.target.value)}
        />
      </div>
      <div className="resource-workspace-settings-layout">
        <div>
          {appService && (
            <>
              <PublicAccessEditor
                appService={appService}
                workspaceSlug={workspaceSlug}
                projectSlug={projectSlug}
                onToast={onToast}
                onAppServiceUpdated={onAppServiceUpdated}
              />
              <DatabaseAttachmentEditor
                appService={appService}
                workspaceSlug={workspaceSlug}
                projectSlug={projectSlug}
                onToast={onToast}
                onAppServiceUpdated={onAppServiceUpdated}
              />
            </>
          )}
          {visibleSections.length ? (
            visibleSections.map((section) => (
              <article
                key={section.id}
                id={section.id}
                className="resource-workspace-setting-section"
              >
                <h3>{section.title}</h3>
                <p>{section.description}</p>
                {section.id === "auto-deploy" && appService ? (
                  <div className="resource-workspace-setting-block">
                    <AutoDeployEditor
                      key={`${appService.id}:${appService.autoDeployEnabled ? "on" : "off"}`}
                      appService={appService}
                      workspaceSlug={workspaceSlug}
                      projectSlug={projectSlug}
                      onToast={onToast}
                      onAppServiceUpdated={onAppServiceUpdated}
                    />
                  </div>
                ) : (
                  <div className="resource-workspace-setting-block">
                    <dl>
                      {section.rows.map(([label, value]) => (
                        <div key={label}>
                          <dt>{label}</dt>
                          <dd>{value}</dd>
                        </div>
                      ))}
                    </dl>
                    {section.id === "networking" && appService ? (
                      <AppPortEditor
                        key={`${appService.id}:${appService.appPort}`}
                        appService={appService}
                        workspaceSlug={workspaceSlug}
                        projectSlug={projectSlug}
                        onToast={onToast}
                        onAppServiceUpdated={onAppServiceUpdated}
                      />
                    ) : (
                      <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() =>
                          onToast(`${section.title} is read-only in development`)
                        }
                      >
                        Manage
                      </Button>
                    )}
                  </div>
                )}
              </article>
            ))
          ) : (
            <ResourceEmptyState
              icon={<Settings2Icon aria-hidden="true" />}
              title="No settings match"
              description="Try a different setting name."
            />
          )}
        </div>
        <nav
          className="resource-workspace-settings-nav"
          aria-label="Settings sections"
        >
          {sections.map((section) => (
            <a key={section.id} href={`#${section.id}`}>
              {section.title}
            </a>
          ))}
        </nav>
      </div>
    </section>
  )
}

function ResourceEmptyState({
  icon,
  title,
  description,
  action,
}: {
  icon: ReactNode
  title: string
  description: string
  action?: ReactNode
}) {
  return (
    <div className="resource-workspace-empty-state">
      <span className="resource-workspace-empty-icon">{icon}</span>
      <h3>{title}</h3>
      <p>{description}</p>
      {action}
    </div>
  )
}
