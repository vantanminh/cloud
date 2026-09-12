import { useEffect, useRef, useState, type ReactNode } from "react"
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
import type { PostgresResource } from "@/lib/types"

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
  onClose: () => void
  onCopyConnectionString: (value: string) => void
  copiedConnectionString: boolean
  onToast: (message: string) => void
  onOpenLogs: () => void
}

export function ResourceWorkspace({
  node,
  environment,
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
  connectionString,
  copiedConnectionString,
  onCopyConnectionString,
  onToast,
}: {
  node: ResourceWorkspaceNode
  connectionString?: string
  copiedConnectionString: boolean
  onCopyConnectionString: (value: string) => void
  onToast: (message: string) => void
}) {
  const [view, setView] = useState<"data" | "stats" | "config">("data")
  const [search, setSearch] = useState("")
  const visibleTables = [] as string[]
  const hasTables = visibleTables.some((table) =>
    table.toLowerCase().includes(search.toLowerCase())
  )

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
                onClick={() => onToast("Tables refreshed")}
              >
                <RefreshCwIcon />
              </Button>
              <Button
                type="button"
                variant="outline"
                className="resource-workspace-accent-button"
                onClick={() => onToast("New table is coming soon")}
              >
                <PlusIcon data-icon="inline-start" />
                New Table
              </Button>
            </div>
          </div>
          <div className="resource-workspace-sql-bar">
            {node.resource
              ? "SELECT * FROM your_table"
              : "SELECT * FROM service_data"}
          </div>
          {hasTables ? (
            <div className="resource-workspace-table-grid">
              {visibleTables
                .filter((table) =>
                  table.toLowerCase().includes(search.toLowerCase())
                )
                .map((table) => (
                  <button
                    key={table}
                    type="button"
                    className="resource-workspace-table-card"
                  >
                    <Table2Icon aria-hidden="true" />
                    <span>{table}</span>
                  </button>
                ))}
            </div>
          ) : (
            <ResourceEmptyState
              icon={<Table2Icon aria-hidden="true" />}
              title={search ? "No tables match" : "No tables yet"}
              description={
                search
                  ? "Try a different table name."
                  : "Create a table from your application or the New Table action when schema editing is enabled."
              }
            />
          )}
        </div>
      )}
      {view === "stats" && (
        <ResourceEmptyState
          icon={<ActivityIcon aria-hidden="true" />}
          title="Stats are warming up"
          description="Connections, cache hit ratio, and table sizes appear after the next metrics scrape."
        />
      )}
      {view === "config" && (
        <ResourceEmptyState
          icon={<Settings2Icon aria-hidden="true" />}
          title="Managed configuration"
          description="Connection settings are managed by the project environment and shown in Settings."
        />
      )}
    </section>
  )
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
