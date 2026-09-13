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
import { Input } from "@/components/ui/input"
import { Label } from "@/components/ui/label"
import { Textarea } from "@/components/ui/textarea"
import { ApiError } from "@/lib/api"
import {
  createDatabaseTable,
  executeDatabaseQuery,
  getDatabaseConfig,
  getDatabaseMetrics,
  getDatabaseStats,
  getDatabaseTableData,
  listDatabaseTables,
  updateAppService,
} from "@/lib/resources"
import type {
  DatabaseConfig,
  DatabaseMetricPoint,
  DatabaseMetrics,
  DatabaseMetricsRange,
  DatabaseStats,
  DatabaseTable,
  DatabaseTableData,
  DatabaseQueryResult,
  AppService,
  PostgresResource,
} from "@/lib/types"

import "./resource-workspace.css"

type ResourceWorkspaceNode = {
  id: "postgres" | "project"
  title: string
  subtitle?: string
  type: string
  volume: string
  status: string
  resource?: PostgresResource | AppService
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
    node.resource?.resourceType === "postgres"
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
              className={cn("resource-workspace-logo", node.id)}
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
              onToast={onToast}
              onOpenLogs={onOpenLogs}
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
            <ConsolePane node={node} onToast={onToast} />
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
  onToast,
  onOpenLogs,
}: {
  node: ResourceWorkspaceNode
  onToast: (message: string) => void
  onOpenLogs: () => void
}) {
  const isReady =
    node.resource?.status === "ready" || (node.id === "project" && !node.resource)
  const status = isReady
    ? "ACTIVE"
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
            ? "This database is isolated to the current project and ready for application connections."
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
            : appService?.serviceUrl ?? "No container deployed"}
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
                  : appService?.status === "error"
                    ? (appService.errorMessage ?? "Deployment failed")
                    : "Deploy an image to start this service"}
              </div>
            </div>
          </div>
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={onOpenLogs}
          >
            <FileTextIcon data-icon="inline-start" />
            View logs
          </Button>
        </div>
        <div className="resource-workspace-deploy-bottom">
          <span>
            {isReady
              ? appService?.serviceUrl
                ? `Service URL · ${appService.serviceUrl}`
                : "Deployment successful"
              : appService?.status === "error"
                ? "Deployment failed — open Add to retry"
                : "Deployment not started"}
          </span>
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
      {appService?.databaseConnection && (
        <article className="resource-workspace-private-link" role="status">
          <span
            className="resource-workspace-private-link-icon"
            aria-hidden="true"
          >
            <DatabaseIcon />
          </span>
          <div>
            <strong>Postgres connected automatically</strong>
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
            CONNECTED
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
              ? "Database provisioned"
              : appService
                ? "App service deployed"
                : "Project created"}
          </strong>
          <div className="resource-workspace-muted">
            Current environment · Knotree Cloud
          </div>
        </div>
        <span className="resource-workspace-history-status">READY</span>
      </article>
    </section>
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
}: {
  node: ResourceWorkspaceNode
  workspaceSlug: string
  projectSlug: string
  connectionString?: string
  copiedConnectionString: boolean
  onCopyConnectionString: (value: string) => void
  onToast: (message: string) => void
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
          title="Database is provisioning"
          description="Database management becomes available as soon as the dedicated PostgreSQL cluster is ready."
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
  const isReady = node.id === "postgres" && node.resource?.status === "ready"
  const [metrics, setMetrics] = useState<DatabaseMetrics | null>(null)
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
        const nextMetrics = await getDatabaseMetrics(
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
    [isReady, projectSlug, range, resourceId, workspaceSlug]
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

  if (node.id !== "postgres" || !resourceId) {
    return (
      <section
        className="resource-workspace-pane"
        id="resource-pane-metrics"
        role="tabpanel"
        aria-labelledby="resource-tab-metrics"
      >
        <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title="No database metrics"
          description="Metrics are available for a dedicated PostgreSQL resource."
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
          title="Database is provisioning"
          description="Live metrics become available as soon as the dedicated PostgreSQL cluster is ready."
        />
      </section>
    )
  }

  const points = metrics?.points ?? []
  const current = points[points.length - 1] ?? null
  const selectedRangeLabel =
    METRIC_RANGE_OPTIONS.find((option) => option.value === range)?.label ??
    "Last 24 hours"
  const lastUpdated = current
    ? new Date(current.timestamp * 1_000).toLocaleTimeString([], {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
      })
    : null

  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-metrics"
      role="tabpanel"
      aria-labelledby="resource-tab-metrics"
    >
      <div className="resource-workspace-metric-toolbar">
        <div className="resource-workspace-metric-status">
          <span className="resource-workspace-muted">
            {selectedRangeLabel}
            {lastUpdated ? ` · updated ${lastUpdated}` : ""}
          </span>
          <span className="resource-workspace-muted">
            Up to {Math.round((metrics?.retentionSeconds ?? 30 * 24 * 60 * 60) / (24 * 60 * 60))} days
          </span>
          {refreshing && (
            <span className="resource-workspace-muted">Refreshing…</span>
          )}
        </div>
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
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-pressed={live}
          onClick={() => {
            setLive((currentLive) => {
              const nextLive = !currentLive
              onToast(nextLive ? "Live metrics resumed" : "Live metrics paused")
              return nextLive
            })
          }}
        >
          <span
            className={cn("resource-workspace-live-dot", !live && "paused")}
            aria-hidden="true"
          />
          {live ? "Live" : "Paused"}
        </Button>
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
          description="Reading live runtime metrics from this project database."
        />
      ) : (
        <div className="resource-workspace-metrics-grid">
          <MetricCard
            title="CPU"
            legend="Container usage"
            accent="blue"
            points={points}
            rangeLabel={selectedRangeLabel}
            series={[
              {
                label: "CPU",
                colorClass: "blue",
                getValue: (point) => point.cpuPercent,
                formatValue: (value) => `${value.toFixed(2)}%`,
              },
            ]}
            scaleMax={100}
            axisMaxLabel="100%"
          />
          <MetricCard
            title="Memory"
            legend="Container usage"
            accent="violet"
            points={points}
            rangeLabel={selectedRangeLabel}
            series={[
              {
                label: "Used",
                colorClass: "violet",
                getValue: (point) => memoryPercent(point),
                formatValue: (value) => `${value.toFixed(1)}%`,
                getDisplayValue: (point) =>
                  point.memoryUsedBytes !== null &&
                  point.memoryLimitBytes !== null
                    ? `${formatBytes(point.memoryUsedBytes)} / ${formatBytes(point.memoryLimitBytes)}`
                    : "Unavailable",
              },
            ]}
            scaleMax={100}
            axisMaxLabel="100%"
          />
          <MetricCard
            title="Volume"
            legend="Used · Capacity"
            accent="ink"
            points={points}
            rangeLabel={selectedRangeLabel}
            series={[
              {
                label: "Used",
                colorClass: "ink",
                getValue: (point) => volumePercent(point),
                formatValue: (value) => `${value.toFixed(1)}%`,
                getDisplayValue: (point) =>
                  point.volumeUsedBytes !== null &&
                  point.volumeCapacityBytes !== null
                    ? `${formatBytes(point.volumeUsedBytes)} / ${formatBytes(point.volumeCapacityBytes)}`
                    : point.volumeUsedBytes !== null
                      ? formatBytes(point.volumeUsedBytes)
                      : "Unavailable",
              },
            ]}
            scaleMax={100}
            axisMaxLabel="100%"
          />
          <MetricCard
            title="Network I/O"
            legend="RX · TX totals"
            accent="green"
            points={points}
            rangeLabel={selectedRangeLabel}
            series={[
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
            ]}
          />
          <MetricCard
            title="Disk I/O"
            legend="Read · Write totals"
            accent="orange"
            points={points}
            rangeLabel={selectedRangeLabel}
            series={[
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
            ]}
          />
        </div>
      )}
    </section>
  )
}

type MetricSeries = {
  label: string
  colorClass: "blue" | "violet" | "ink" | "green" | "orange"
  getValue: (point: DatabaseMetricPoint) => number | null
  formatValue: (value: number) => string
  getDisplayValue?: (point: DatabaseMetricPoint) => string
}

function MetricCard({
  title,
  legend,
  accent,
  points,
  series,
  rangeLabel,
  scaleMax,
  axisMaxLabel,
}: {
  title: string
  legend: string
  accent: "blue" | "violet" | "ink" | "green" | "orange"
  points: DatabaseMetricPoint[]
  series: MetricSeries[]
  rangeLabel: string
  scaleMax?: number
  axisMaxLabel?: string
}) {
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
  const chartBottom = 160
  const chartHeight = 140
  const xForIndex = (index: number) =>
    chartLeft +
    (points.length > 1 ? (index * chartWidth) / (points.length - 1) : chartWidth / 2)
  const yForValue = (value: number) =>
    chartBottom -
    (Math.max(0, Math.min(value, chartMax)) / chartMax) * chartHeight
  const axisLabel = axisMaxLabel ?? formatBytes(chartMax)
  const hoveredPoint =
    hoveredIndex === null ? null : (points[hoveredIndex] ?? null)
  const hoveredX = hoveredIndex === null ? null : xForIndex(hoveredIndex)

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
      points.length === 1
        ? 0
        : Math.round(progress * (points.length - 1))
    setHoveredIndex(index)
  }

  return (
    <article className={cn("resource-workspace-metric-card", accent)}>
      <span className="resource-workspace-metric-legend">● {legend}</span>
      <h3>{title}</h3>
      <div className="resource-workspace-metric-values">
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
          viewBox="0 0 360 180"
          preserveAspectRatio="none"
          role="img"
          aria-label={`${title} usage, ${rangeLabel.toLowerCase()}`}
          onMouseMove={handleChartMove}
          onMouseLeave={() => setHoveredIndex(null)}
        >
          <g className="resource-workspace-chart-grid">
            <line x1="40" y1="20" x2="350" y2="20" />
            <line x1="40" y1="55" x2="350" y2="55" />
            <line x1="40" y1="90" x2="350" y2="90" />
            <line x1="40" y1="125" x2="350" y2="125" />
            <line x1="40" y1="160" x2="350" y2="160" />
          </g>
          <text x="0" y="24">
            {axisLabel}
          </text>
          <text x="0" y="164">
            0
          </text>
          {hoveredPoint && hoveredX !== null && (
            <line
              className="resource-workspace-chart-hover-line"
              x1={hoveredX}
              y1="20"
              x2={hoveredX}
              y2="160"
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
                <polyline
                  key={item.label}
                  className={cn(
                    "resource-workspace-chart-line",
                    item.colorClass
                  )}
                  points={linePoints}
                />
              ) : null
            })}
          {hoveredPoint && hoveredX !== null &&
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
                  r="4"
                />
              )
            })}
        </svg>
        {hoveredPoint && hoveredX !== null && (
          <div
            className="resource-workspace-chart-tooltip"
            role="status"
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
  point: DatabaseMetricPoint
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

function memoryPercent(point: DatabaseMetricPoint) {
  if (
    point.memoryUsedBytes === null ||
    point.memoryLimitBytes === null ||
    point.memoryLimitBytes <= 0
  ) {
    return null
  }
  return Math.min(100, (point.memoryUsedBytes / point.memoryLimitBytes) * 100)
}

function volumePercent(point: DatabaseMetricPoint) {
  if (
    point.volumeUsedBytes === null ||
    point.volumeCapacityBytes === null ||
    point.volumeCapacityBytes <= 0
  ) {
    return null
  }
  return Math.min(100, (point.volumeUsedBytes / point.volumeCapacityBytes) * 100)
}

function ConsolePane({
  node,
  onToast,
}: {
  node: ResourceWorkspaceNode
  onToast: (message: string) => void
}) {
  const isAppService = node.resource?.resourceType === "app"
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
            Project console
          </span>
          <span className="resource-workspace-muted">Not connected</span>
        </div>
        <ResourceEmptyState
          icon={<ServerIcon aria-hidden="true" />}
          title={isAppService ? "Container console is unavailable" : "Console is unavailable"}
          description={
            isAppService
              ? "App service containers are managed through Docker. Use the service URL to inspect the running application."
              : "This managed PostgreSQL resource exposes connection credentials, not a shell."
          }
        />
        <Button
          type="button"
          variant="outline"
          onClick={() =>
            onToast(
              isAppService
                ? "Container logs opened"
                : "Connection details opened"
            )
          }
        >
          <CopyIcon data-icon="inline-start" />
          {isAppService ? "View container details" : "View connection details"}
        </Button>
      </div>
    </section>
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

  useEffect(() => {
    setAppPort(String(appService.appPort))
    setError(null)
  }, [appService.id, appService.appPort])

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
      const resource = await updateAppService(workspaceSlug, projectSlug, {
        appPort: parsedPort,
      })
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
                ["Public URL", appService.serviceUrl ?? "Pending"],
                ["Container", appService.containerName ?? "Pending"],
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
          {visibleSections.length ? (
            visibleSections.map((section) => (
              <article
                key={section.id}
                id={section.id}
                className="resource-workspace-setting-section"
              >
                <h3>{section.title}</h3>
                <p>{section.description}</p>
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
}: {
  icon: ReactNode
  title: string
  description: string
}) {
  return (
    <div className="resource-workspace-empty-state">
      <span className="resource-workspace-empty-icon">{icon}</span>
      <h3>{title}</h3>
      <p>{description}</p>
    </div>
  )
}
