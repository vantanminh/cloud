import {
  useCallback,
  useEffect,
  useRef,
  useState,
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
import { Textarea } from "@/components/ui/textarea"
import { ApiError } from "@/lib/api"
import {
  createDatabaseTable,
  executeDatabaseQuery,
  getDatabaseConfig,
  getDatabaseStats,
  getDatabaseTableData,
  listDatabaseTables,
} from "@/lib/resources"
import type {
  DatabaseConfig,
  DatabaseStats,
  DatabaseTable,
  DatabaseTableData,
  DatabaseQueryResult,
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
  resource?: PostgresResource
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
}

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

  const connectionString = node.resource?.connectionString ?? undefined

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
          {activeTab === "metrics" && <MetricsPane onToast={onToast} />}
          {activeTab === "console" && <ConsolePane onToast={onToast} />}
          {activeTab === "settings" && (
            <SettingsPane
              node={node}
              environment={environment}
              onToast={onToast}
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
  const isReady = node.resource?.status === "ready" || node.id === "project"
  const status = isReady
    ? "ACTIVE"
    : (node.resource?.status.toUpperCase() ?? "PENDING")

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
            : "This service is the application entry point for the current project."}
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
          {node.id === "postgres" ? "Private database" : "Project service"}
        </span>
        <span>
          {node.resource
            ? `${node.resource.host} · ${node.resource.port}`
            : "1 replica"}
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
                {node.resource
                  ? `PostgreSQL · ${node.resource.databaseName}`
                  : node.title}
              </strong>
              <div className="resource-workspace-muted">
                {isReady
                  ? "Ready to accept connections"
                  : "Provisioning resource"}
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
          {isReady ? "Deployment successful" : "Deployment in progress"}
        </div>
      </article>
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
            {node.resource ? "Database provisioned" : "Project created"}
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
  const [tables, setTables] = useState<DatabaseTable[]>([])
  const [selectedTable, setSelectedTable] = useState<DatabaseTable | null>(null)
  const [tableOffset, setTableOffset] = useState(0)
  const [tableData, setTableData] = useState<DatabaseTableData | null>(null)
  const [stats, setStats] = useState<DatabaseStats | null>(null)
  const [config, setConfig] = useState<DatabaseConfig[]>([])
  const [query, setQuery] = useState("SELECT * FROM public.your_table LIMIT 50")
  const [queryResult, setQueryResult] = useState<DatabaseQueryResult | null>(
    null
  )
  const [loading, setLoading] = useState(true)
  const [queryRunning, setQueryRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [createOpen, setCreateOpen] = useState(false)
  const resourceId = node.resource?.id
  const isReady = node.resource?.status === "ready"

  const loadTables = useCallback(async () => {
    if (!resourceId || !isReady) {
      return
    }
    setLoading(true)
    setError(null)
    try {
      const nextTables = await listDatabaseTables(
        workspaceSlug,
        projectSlug,
        resourceId,
        search
      )
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
    } catch (requestError) {
      setError(databaseErrorMessage(requestError))
    } finally {
      setLoading(false)
    }
  }, [isReady, projectSlug, resourceId, search, workspaceSlug])

  useEffect(() => {
    if (!resourceId || !isReady) {
      return
    }
    let active = true
    void listDatabaseTables(
      workspaceSlug,
      projectSlug,
      resourceId,
      search
    )
      .then((nextTables) => {
        if (!active) {
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
  }, [isReady, projectSlug, resourceId, search, workspaceSlug])

  useEffect(() => {
    if (!resourceId || !isReady || !selectedTable || view !== "data") {
      return
    }
    let active = true
    void getDatabaseTableData(
      workspaceSlug,
      projectSlug,
      resourceId,
      selectedTable.tableName,
      selectedTable.schemaName,
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
  }, [isReady, projectSlug, resourceId, selectedTable, tableOffset, view, workspaceSlug])

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
    setSearch("")
    setSelectedTable(null)
    setTableOffset(0)
    setTableData(null)
    void loadTables()
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
            onClick={() => setView(item)}
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
                  onChange={(event) => setSearch(event.target.value)}
                />
              </div>
            </div>
            <div className="resource-workspace-toolbar-actions">
              <Button
                type="button"
                variant="outline"
                size="icon-sm"
                aria-label="Refresh tables"
                onClick={() => void loadTables()}
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
          {queryResult && (
            <DatabaseQueryResultView result={queryResult} />
          )}
          {selectedTable && tableData ? (
            <DatabaseTableView
              data={tableData}
              loading={loading}
              onPageChange={setTableOffset}
              onBack={() => {
                setSelectedTable(null)
                setTableData(null)
              }}
            />
          ) : tables.length > 0 ? (
            <div className="resource-workspace-table-grid">
              {tables.map((table) => (
                  <button
                    key={`${table.schemaName}.${table.tableName}`}
                    type="button"
                    className="resource-workspace-table-card"
                    onClick={() => {
                      setQueryResult(null)
                      setSelectedTable(table)
                      setTableOffset(0)
                    }}
                  >
                    <Table2Icon aria-hidden="true" />
                    <span>{table.tableName}</span>
                    <small>
                      {table.schemaName} · {formatRowCount(table.estimatedRows)} rows
                    </small>
                  </button>
                ))}
            </div>
          ) : (
            <ResourceEmptyState
              icon={<Table2Icon aria-hidden="true" />}
              title={loading ? "Loading tables…" : search ? "No tables match" : "No tables yet"}
              description={
                search
                  ? "Try a different table name."
                  : "Create a table here or connect your application to this dedicated PostgreSQL cluster."
              }
            />
          )}
        </div>
      )}
      {view === "stats" && (
        stats ? <DatabaseStatsView stats={stats} /> : <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title={loading ? "Loading stats…" : "No stats available"}
          description="Live statistics are read directly from this project database."
        />
      )}
      {view === "config" && (
        config.length > 0 ? <DatabaseConfigView config={config} /> : <ResourceEmptyState
          icon={<Settings2Icon aria-hidden="true" />}
          title={loading ? "Loading config…" : "No config available"}
          description="The live PostgreSQL settings are read from this project database."
        />
      )}
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
          Rows {data.rowCount === 0 ? 0 : data.offset + 1}–{data.offset + data.rowCount}
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
        Query completed in {result.durationMs}ms · {result.affectedRows} row(s) affected.
      </div>
    )
  }
  return (
    <div className="resource-workspace-query-result">
      <div className="resource-workspace-data-head">
        <strong>Query result</strong>
        <span className="resource-workspace-muted">
          {result.rowCount} rows · {result.durationMs}ms{result.truncated ? " · result truncated" : ""}
        </span>
      </div>
      <div className="resource-workspace-data-table-wrap">
        <table className="resource-workspace-data-table">
          <thead>
            <tr>
              {result.columns.map((column) => (
                <th key={column} scope="col">{column}</th>
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
          <button type="button" className="resource-workspace-close" aria-label="Close new table" onClick={onCancel}>
            <XIcon aria-hidden="true" />
          </button>
        </div>
        <label className="resource-workspace-form-field">
          Table name
          <Input value={name} onChange={(event) => setName(event.target.value)} required autoFocus />
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
                  { name: `column_${current.length + 1}`, dataType: "text", primaryKey: false, nullable: true },
                ])
              }
              disabled={columns.length >= 50}
            >
              <PlusIcon data-icon="inline-start" /> Add column
            </Button>
          </div>
          {columns.map((column, index) => (
            <div className="resource-workspace-column-row" key={`${index}-${column.name}`}>
              <Input
                aria-label={`Column ${index + 1} name`}
                value={column.name}
                onChange={(event) =>
                  setColumns((current) => current.map((item, itemIndex) => itemIndex === index ? { ...item, name: event.target.value } : item))
                }
                required
              />
              <select
                aria-label={`Column ${index + 1} type`}
                value={column.dataType}
                onChange={(event) =>
                  setColumns((current) => current.map((item, itemIndex) => itemIndex === index ? { ...item, dataType: event.target.value } : item))
                }
              >
                {["text", "bigint", "integer", "boolean", "numeric", "date", "timestamptz", "uuid", "jsonb", "bytea"].map((type) => <option key={type} value={type}>{type}</option>)}
              </select>
              <label className="resource-workspace-checkbox">
                <input
                  type="checkbox"
                  checked={column.nullable}
                  disabled={column.primaryKey}
                  onChange={(event) =>
                    setColumns((current) => current.map((item, itemIndex) => itemIndex === index ? { ...item, nullable: event.target.checked } : item))
                  }
                />
                Nullable
              </label>
              <label className="resource-workspace-checkbox">
                <input
                  type="checkbox"
                  checked={column.primaryKey}
                  onChange={(event) =>
                    setColumns((current) => current.map((item, itemIndex) => itemIndex === index ? { ...item, primaryKey: event.target.checked, nullable: event.target.checked ? false : item.nullable } : item))
                  }
                />
                PK
              </label>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                aria-label={`Remove column ${index + 1}`}
                onClick={() => setColumns((current) => current.filter((_, itemIndex) => itemIndex !== index))}
                disabled={columns.length === 1}
              >
                <XIcon />
              </Button>
            </div>
          ))}
        </div>
        <div className="resource-workspace-modal-actions">
          <Button type="button" variant="outline" onClick={onCancel}>Cancel</Button>
          <Button type="submit" disabled={saving || !name.trim()}>{saving ? "Creating…" : "Create table"}</Button>
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
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`
}

function formatRowCount(value: number) {
  return new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 }).format(value)
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
  const variables = node.resource
    ? [
        ["DATABASE_URL", "postgres://••••••••"],
        ["PGDATABASE", node.resource.databaseName],
        ["PGHOST", node.resource.host],
        ["PGPORT", String(node.resource.port)],
        ["PGUSER", node.resource.username],
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

function MetricsPane({ onToast }: { onToast: (message: string) => void }) {
  return (
    <section
      className="resource-workspace-pane"
      id="resource-pane-metrics"
      role="tabpanel"
      aria-labelledby="resource-tab-metrics"
    >
      <div className="resource-workspace-metric-toolbar">
        <span className="resource-workspace-muted">Last 24 hours</span>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => onToast("Metrics range opened")}
        >
          1 day
        </Button>
        <Button
          type="button"
          variant="outline"
          size="sm"
          aria-pressed="true"
          onClick={() => onToast("Live metrics resumed")}
        >
          <span className="resource-workspace-live-dot" aria-hidden="true" />
          Live
        </Button>
      </div>
      <div className="resource-workspace-metrics-grid">
        <MetricCard title="CPU" legend="Sum · Replicas" accent="blue" />
        <MetricCard title="Memory" legend="Sum · Replicas" accent="violet" />
        <MetricCard title="Volume" legend="Used · Capacity" accent="ink" />
      </div>
    </section>
  )
}

function MetricCard({
  title,
  legend,
  accent,
}: {
  title: string
  legend: string
  accent: "blue" | "violet" | "ink"
}) {
  return (
    <article className="resource-workspace-metric-card">
      <span className="resource-workspace-metric-legend">● {legend}</span>
      <h3>{title}</h3>
      <svg
        viewBox="0 0 360 180"
        role="img"
        aria-label={`${title} usage, last day`}
      >
        <g className="resource-workspace-chart-grid">
          <line x1="40" y1="20" x2="350" y2="20" />
          <line x1="40" y1="55" x2="350" y2="55" />
          <line x1="40" y1="90" x2="350" y2="90" />
          <line x1="40" y1="125" x2="350" y2="125" />
          <line x1="40" y1="160" x2="350" y2="160" />
        </g>
        <text x="0" y="24">
          100%
        </text>
        <text x="0" y="164">
          0%
        </text>
        <polyline
          className={cn("resource-workspace-chart-line", accent)}
          points={
            accent === "violet"
              ? "40,160 110,160 150,142 198,148 240,102 290,116 350,96"
              : accent === "ink"
                ? "40,128 100,124 150,120 200,108 260,112 310,98 350,92"
                : "40,160 120,160 180,158 220,128 280,142 350,138"
          }
        />
      </svg>
    </article>
  )
}

function ConsolePane({ onToast }: { onToast: (message: string) => void }) {
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
      </div>
    </section>
  )
}

function SettingsPane({
  node,
  environment,
  onToast,
}: {
  node: ResourceWorkspaceNode
  environment: string
  onToast: (message: string) => void
}) {
  const [search, setSearch] = useState("")
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
      description: "Connection endpoint and access scope for this resource.",
      rows: node.resource
        ? [
            ["Host", node.resource.host],
            ["Port", String(node.resource.port)],
            ["Database", node.resource.databaseName],
            ["Username", node.resource.username],
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
  const visibleSections = sections.filter((section) =>
    `${section.title} ${section.description} ${section.rows.flat().join(" ")}`
      .toLowerCase()
      .includes(search.toLowerCase())
  )

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
