import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import {
  ActivityIcon,
  BarChart3Icon,
  BoxIcon,
  BellIcon,
  ChevronDownIcon,
  DatabaseIcon,
  FileTextIcon,
  GitBranchIcon,
  HardDriveIcon,
  Layers3Icon,
  Maximize2Icon,
  NetworkIcon,
  Redo2Icon,
  SearchIcon,
  Settings2Icon,
  MoonIcon,
  SunIcon,
  Undo2Icon,
  XIcon,
  ZoomInIcon,
  ZoomOutIcon,
} from "lucide-react"
import { cn } from "cn"
import { useNavigate, useParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { AppServiceCreateDialog } from "@/components/app-service-create-dialog"
import { PostgresCreateDialog } from "@/components/postgres-create-dialog"
import { ResourceWorkspace } from "@/components/resource-workspace"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { getProject } from "@/lib/projects"
import { listAppServices, listPostgresResources } from "@/lib/resources"
import type {
  AppService,
  AppServiceStatus,
  PostgresResource,
  PostgresResourceStatus,
  Project,
  Workspace,
} from "@/lib/types"

import "./project-home.css"

type TopologyNodeId = "postgres" | "project"
type ConnectorId = "project-postgres"
type Environment = "production" | "staging"
type Theme = "light" | "dark"
type WorkspaceView = "topology" | "logs"

type TopologyNode = {
  id: TopologyNodeId
  title: string
  subtitle?: string
  type: string
  volume: string
  status: string
  resource?: PostgresResource | AppService
  position: { left: number; top: number }
}

type PersistedDashboardState = {
  zoom?: number
  selectedNode?: TopologyNodeId | null
}

export function ProjectHomePage() {
  const { session } = useAuth()
  const { workspaceSlug, projectSlug } = useParams()
  const [project, setProject] = useState<Project | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)

  useEffect(() => {
    if (!workspaceSlug || !projectSlug) {
      return undefined
    }

    let active = true
    void getProject(workspaceSlug, projectSlug)
      .then((nextProject) => {
        if (active) {
          setProject(nextProject)
        }
      })
      .catch((error: unknown) => {
        if (!active) {
          return
        }
        setLoadError(
          error instanceof ApiError
            ? error.message
            : "The API is currently unavailable. Please try again."
        )
      })

    return () => {
      active = false
    }
  }, [projectSlug, workspaceSlug])

  if (!session?.workspace || !workspaceSlug || !projectSlug) {
    return null
  }

  if (loadError) {
    return (
      <ProjectLoadError workspace={session.workspace} message={loadError} />
    )
  }

  if (!project) {
    return (
      <main className="project-load-screen" role="status">
        <Spinner />
        <span>Loading project</span>
      </main>
    )
  }

  return (
    <TopologyDashboard
      project={project}
      workspace={session.workspace}
      workspaceSlug={workspaceSlug}
      projectSlug={projectSlug}
    />
  )
}

function ProjectLoadError({
  workspace,
  message,
}: {
  workspace: Workspace
  message: string
}) {
  const navigate = useNavigate()

  return (
    <main className="project-load-error">
      <div className="project-load-error-mark" aria-hidden="true">
        <XIcon />
      </div>
      <div className="flex max-w-md flex-col gap-3 text-center">
        <h1 className="font-heading text-3xl font-semibold tracking-[-0.04em]">
          Project unavailable
        </h1>
        <p className="leading-7 text-muted-foreground">{message}</p>
      </div>
      <Button onClick={() => navigate(`/workspace/${workspace.slug}`)}>
        Back to workspace
      </Button>
    </main>
  )
}

function TopologyDashboard({
  project,
  workspace,
  workspaceSlug,
  projectSlug,
}: {
  project: Project
  workspace: Workspace
  workspaceSlug: string
  projectSlug: string
}) {
  const navigate = useNavigate()
  const { session, signOut } = useAuth()
  const [postgresResource, setPostgresResource] =
    useState<PostgresResource | null>(null)
  const [appService, setAppService] = useState<AppService | null>(null)
  const [resourcesLoading, setResourcesLoading] = useState(true)
  const [resourceError, setResourceError] = useState<string | null>(null)
  const [postgresDialogOpen, setPostgresDialogOpen] = useState(false)
  const [appServiceDialogOpen, setAppServiceDialogOpen] = useState(false)
  const [copiedConnectionString, setCopiedConnectionString] = useState(false)
  const [theme, setTheme] = useState<Theme>(() =>
    readStoredTheme("project-topology-dashboard-theme")
  )
  const [canvasTheme, setCanvasTheme] = useState<Theme>(() =>
    readStoredTheme("project-topology-canvas-theme")
  )
  const [activeView, setActiveView] = useState<WorkspaceView>("topology")
  const nodes = useMemo<TopologyNode[]>(() => {
    const projectNode: TopologyNode = {
      id: "project",
      title: project.name,
      subtitle: appService?.image ?? project.slug,
      type: "App service",
      volume: appService?.containerName ?? `${project.slug}-volume`,
      status: appService ? resourceStatusLabel(appService.status) : "Needs setup",
      resource: appService ?? undefined,
      position: postgresResource
        ? { left: 42, top: 53 }
        : { left: 50, top: 35 },
    }
    if (!postgresResource) {
      return [projectNode]
    }

    return [
      {
        id: "postgres",
        title: postgresResource.name,
        subtitle: postgresResource.databaseName,
        type: "PostgreSQL database",
        volume: postgresResource.databaseName,
        status: resourceStatusLabel(postgresResource.status),
        resource: postgresResource,
        position: { left: 42, top: 20 },
      },
      projectNode,
    ]
  }, [appService, postgresResource, project.name, project.slug])
  const stateKey = `project-topology-dashboard-state:${project.id}`
  const initialState = useMemo(() => readPersistedState(stateKey), [stateKey])
  const [zoom, setZoom] = useState(initialState.zoom)
  const [selectedNode, setSelectedNode] = useState<TopologyNodeId | null>(
    initialState.selectedNode
  )
  const [layersVisible, setLayersVisible] = useState(false)
  const [environment, setEnvironment] = useState<Environment>("production")
  const [addMenuOpen, setAddMenuOpen] = useState(false)
  const [workspaceMenuOpen, setWorkspaceMenuOpen] = useState(false)
  const [environmentMenuOpen, setEnvironmentMenuOpen] = useState(false)
  const [toast, setToast] = useState<string | null>(null)
  const toastTimer = useRef<number | null>(null)
  const copyTimer = useRef<number | null>(null)
  const addMenuRef = useRef<HTMLDivElement>(null)
  const workspaceMenuRef = useRef<HTMLDivElement>(null)
  const environmentMenuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let active = true
    void Promise.all([
      listPostgresResources(workspaceSlug, projectSlug),
      listAppServices(workspaceSlug, projectSlug),
    ])
      .then(([postgresResources, appServices]) => {
        if (!active) {
          return
        }
        setPostgresResource(postgresResources[0] ?? null)
        setAppService(appServices[0] ?? null)
        setResourcesLoading(false)
      })
      .catch((error: unknown) => {
        if (!active) {
          return
        }
        setResourcesLoading(false)
        setResourceError(
          error instanceof ApiError
            ? error.message
            : "The API is currently unavailable. Please try again."
        )
      })

    return () => {
      active = false
    }
  }, [projectSlug, workspaceSlug])

  const showToast = useCallback((message: string) => {
    if (toastTimer.current !== null) {
      window.clearTimeout(toastTimer.current)
    }
    setToast(message)
    toastTimer.current = window.setTimeout(() => {
      setToast(null)
      toastTimer.current = null
    }, 2600)
  }, [])

  useEffect(() => {
    window.localStorage.setItem(
      stateKey,
      JSON.stringify({ zoom, selectedNode } satisfies PersistedDashboardState)
    )
  }, [selectedNode, stateKey, zoom])

  useEffect(() => {
    window.localStorage.setItem("project-topology-dashboard-theme", theme)
  }, [theme])

  useEffect(() => {
    window.localStorage.setItem("project-topology-canvas-theme", canvasTheme)
  }, [canvasTheme])

  useEffect(() => {
    return () => {
      if (toastTimer.current !== null) {
        window.clearTimeout(toastTimer.current)
      }
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current)
      }
    }
  }, [])

  useEffect(() => {
    function handlePointerDown(event: PointerEvent) {
      const target = event.target as Node | null
      if (!addMenuRef.current?.contains(target)) {
        setAddMenuOpen(false)
      }
      if (!workspaceMenuRef.current?.contains(target)) {
        setWorkspaceMenuOpen(false)
      }
      if (!environmentMenuRef.current?.contains(target)) {
        setEnvironmentMenuOpen(false)
      }
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return
      }
      setAddMenuOpen(false)
      setWorkspaceMenuOpen(false)
      setEnvironmentMenuOpen(false)
    }

    document.addEventListener("pointerdown", handlePointerDown)
    document.addEventListener("keydown", handleKeyDown)
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown)
      document.removeEventListener("keydown", handleKeyDown)
    }
  }, [])

  function updateZoom(nextZoom: number, message: string) {
    const clampedZoom = Math.min(
      1.25,
      Math.max(0.8, Number(nextZoom.toFixed(2)))
    )
    setZoom(clampedZoom)
    showToast(message)
  }

  function selectNode(nodeId: TopologyNodeId) {
    setSelectedNode(nodeId)
  }

  function handlePostgresAdd() {
    setAddMenuOpen(false)
    if (postgresResource?.status === "ready") {
      setSelectedNode("postgres")
      showToast("Postgres is already provisioned")
      return
    }
    setPostgresDialogOpen(true)
  }

  function handlePostgresCreated(resource: PostgresResource) {
    setPostgresResource(resource)
    setResourceError(null)
    setPostgresDialogOpen(false)
    setSelectedNode("postgres")
    showToast("Postgres database is ready")
  }

  function handleAppServiceAdd() {
    setAddMenuOpen(false)
    if (appService?.status === "ready") {
      setSelectedNode("project")
      showToast("App service is already deployed")
      return
    }
    setAppServiceDialogOpen(true)
  }

  function handleAppServiceCreated(resource: AppService) {
    setAppService(resource)
    setResourceError(null)
    setAppServiceDialogOpen(false)
    setSelectedNode("project")
    showToast("App service is deployed")
  }

  async function handleCopyConnectionString(value: string) {
    if (!navigator.clipboard) {
      showToast("Copy is unavailable in this browser")
      return
    }
    try {
      await navigator.clipboard.writeText(value)
      setCopiedConnectionString(true)
      showToast("Connection string copied")
      if (copyTimer.current !== null) {
        window.clearTimeout(copyTimer.current)
      }
      copyTimer.current = window.setTimeout(() => {
        setCopiedConnectionString(false)
        copyTimer.current = null
      }, 2200)
    } catch {
      showToast("Could not copy the connection string")
    }
  }

  const closeResourceWorkspace = useCallback(() => {
    setSelectedNode(null)
  }, [])

  const openResourceLogs = useCallback(() => {
    setSelectedNode(null)
    setActiveView("logs")
  }, [])

  function closeMenus() {
    setAddMenuOpen(false)
    setWorkspaceMenuOpen(false)
    setEnvironmentMenuOpen(false)
  }

  function toggleTheme() {
    setTheme((current) => (current === "dark" ? "light" : "dark"))
  }

  function toggleCanvasTheme() {
    setCanvasTheme((current) => (current === "dark" ? "light" : "dark"))
  }

  async function handleSignOut() {
    try {
      await signOut()
    } finally {
      navigate("/login", { replace: true })
    }
  }

  const selectedNodeData = nodes.find((node) => node.id === selectedNode)
  const memberInitial = getInitial(session?.user.fullName ?? workspace.name)

  return (
    <div className="project-home" data-theme={theme}>
      <header className="project-topbar">
        <div className="project-topbar-left">
          <button
            className="project-brand-button"
            type="button"
            aria-label="Open workspace home"
            onClick={() => navigate(`/workspace/${workspace.slug}`)}
          >
            <span className="project-brand-mark" aria-hidden="true">
              <span />
            </span>
          </button>
          <span className="project-topbar-divider" aria-hidden="true" />
          <div
            ref={workspaceMenuRef}
            className="project-workspace-switcher project-menu-anchor"
          >
            <div className="project-workspace-name">
              <span className="project-avatar-dot" aria-hidden="true">
                {getInitial(workspace.name)}
              </span>
              <button
                className="project-context-trigger"
                type="button"
                aria-expanded={workspaceMenuOpen}
                aria-controls="project-workspace-menu"
                onClick={() => {
                  setWorkspaceMenuOpen((current) => !current)
                  setEnvironmentMenuOpen(false)
                }}
              >
                <span className="project-context-label">{workspace.name}</span>
                <ChevronDownIcon
                  className="project-chevron"
                  aria-hidden="true"
                />
              </button>
            </div>
            <div
              id="project-workspace-menu"
              className="project-context-menu project-workspace-menu"
              hidden={!workspaceMenuOpen}
            >
              <div className="project-menu-heading">Workspace</div>
              <button
                type="button"
                className="project-context-menu-item is-current"
                onClick={() => {
                  closeMenus()
                  showToast(`${workspace.name} selected`)
                }}
              >
                {workspace.name}
              </button>
              <button
                type="button"
                className="project-context-menu-item"
                onClick={() => {
                  closeMenus()
                  showToast("Additional workspaces are coming soon")
                }}
              >
                Workspace settings
              </button>
            </div>
          </div>
          <div
            ref={environmentMenuRef}
            className="project-environment-switcher project-menu-anchor"
          >
            <button
              className="project-context-trigger project-environment-trigger"
              type="button"
              aria-expanded={environmentMenuOpen}
              aria-controls="project-environment-menu"
              onClick={() => {
                setEnvironmentMenuOpen((current) => !current)
                setWorkspaceMenuOpen(false)
              }}
            >
              <span className="project-context-label">{environment}</span>
              <ChevronDownIcon className="project-chevron" aria-hidden="true" />
            </button>
            <div
              id="project-environment-menu"
              className="project-context-menu project-environment-menu"
              hidden={!environmentMenuOpen}
            >
              <div className="project-menu-heading">Environment</div>
              {(["production", "staging"] as Environment[]).map((option) => (
                <button
                  key={option}
                  type="button"
                  className={cn(
                    "project-context-menu-item",
                    option === environment && "is-current"
                  )}
                  onClick={() => {
                    setEnvironment(option)
                    closeMenus()
                    showToast(`${option} selected`)
                  }}
                >
                  {option}
                </button>
              ))}
            </div>
          </div>
        </div>

        <div className="project-topbar-right">
          <div className="project-topbar-actions" aria-label="Utility actions">
            <button
              className="project-icon-button"
              type="button"
              aria-label={
                theme === "dark"
                  ? "Switch to light mode"
                  : "Switch to dark mode"
              }
              aria-pressed={theme === "dark"}
              onClick={toggleTheme}
            >
              {theme === "dark" ? (
                <MoonIcon aria-hidden="true" />
              ) : (
                <SunIcon aria-hidden="true" />
              )}
            </button>
            <button
              className="project-icon-button"
              type="button"
              aria-label="Open activity"
              onClick={() => showToast("Activity is coming soon")}
            >
              <ActivityIcon aria-hidden="true" />
            </button>
            <button
              className="project-icon-button"
              type="button"
              aria-label="Open notifications"
              onClick={() => showToast("Notifications are coming soon")}
            >
              <BellIcon aria-hidden="true" />
            </button>
          </div>
          <div className="project-billing-badge" aria-label="Trial status">
            <strong>30 days</strong>&nbsp;or $4.99 left
          </div>
          <button
            className="project-agent-button"
            type="button"
            aria-label="Open agent"
            onClick={() => showToast("Agent is coming soon")}
          >
            <span className="project-agent-avatar" aria-hidden="true">
              {memberInitial}
            </span>
            <span>Agent</span>
          </button>
        </div>
      </header>

      <aside className="project-side-rail">
        <nav className="project-rail-nav" aria-label="Primary">
          <ProjectRailButton
            active={activeView === "topology"}
            label="Topology"
            onClick={() => setActiveView("topology")}
          >
            <NetworkIcon aria-hidden="true" />
          </ProjectRailButton>
          <ProjectRailButton
            label="Metrics"
            onClick={() => showToast("Metrics is available in this workspace")}
          >
            <BarChart3Icon aria-hidden="true" />
          </ProjectRailButton>
          <ProjectRailButton
            active={activeView === "logs"}
            label="Logs"
            onClick={() => {
              setSelectedNode(null)
              setActiveView("logs")
            }}
          >
            <FileTextIcon aria-hidden="true" />
          </ProjectRailButton>
          <ProjectRailButton
            label="Resources"
            onClick={() =>
              showToast("Resources is available in this workspace")
            }
          >
            <BoxIcon aria-hidden="true" />
          </ProjectRailButton>
          <ProjectRailButton
            label="Settings"
            onClick={() => navigate("/settings/integrations")}
          >
            <Settings2Icon aria-hidden="true" />
          </ProjectRailButton>
        </nav>
        <button
          className="project-account-button"
          type="button"
          aria-label="Sign out"
          onClick={() => void handleSignOut()}
        >
          {memberInitial}
        </button>
      </aside>

      <main className="project-dashboard-main">
        {activeView === "logs" ? (
          <LogsWorkspace project={project} environment={environment} />
        ) : (
          <>
            <h1 className="sr-only">{project.name} infrastructure topology</h1>
            <section
              className={cn(
                "project-topology-shell",
                selectedNode && "has-selection",
                layersVisible && "has-layer-guidance"
              )}
              aria-label={`${environment} infrastructure topology for ${project.name}`}
              data-canvas-theme={canvasTheme}
            >
              <div className="project-canvas-toolbar">
                <button
                  className="project-canvas-theme-toggle"
                  type="button"
                  aria-label={
                    canvasTheme === "dark"
                      ? "Switch canvas to light mode"
                      : "Switch canvas to dark mode"
                  }
                  aria-pressed={canvasTheme === "dark"}
                  onClick={toggleCanvasTheme}
                >
                  {canvasTheme === "dark" ? (
                    <MoonIcon aria-hidden="true" />
                  ) : (
                    <SunIcon aria-hidden="true" />
                  )}
                </button>
                <div ref={addMenuRef} className="project-add-wrap">
                  <button
                    className="project-primary-button"
                    type="button"
                    aria-expanded={addMenuOpen}
                    aria-controls="project-add-menu"
                    onClick={() => {
                      setAddMenuOpen((current) => !current)
                      setWorkspaceMenuOpen(false)
                      setEnvironmentMenuOpen(false)
                    }}
                  >
                    <span className="project-plus" aria-hidden="true">
                      +
                    </span>
                    <span>Add</span>
                  </button>
                  <div
                    id="project-add-menu"
                    className="project-add-menu"
                    hidden={!addMenuOpen}
                  >
                    <div className="project-menu-heading">Add resource</div>
                    <ProjectAddOption
                      mark="P"
                      label={
                        postgresResource?.status === "ready"
                          ? "Postgres (ready)"
                          : "Postgres"
                      }
                      onClick={handlePostgresAdd}
                    />
                    <ProjectAddOption
                      mark="R"
                      label="Redis"
                      onClick={() => {
                        setAddMenuOpen(false)
                        showToast("Redis provisioning is coming soon")
                      }}
                    />
                    <ProjectAddOption
                      mark="S"
                      label="App service"
                      onClick={handleAppServiceAdd}
                    />
                  </div>
                </div>
              </div>

              <svg
                className="project-connector-layer"
                viewBox="0 0 100 100"
                preserveAspectRatio="none"
                aria-hidden="true"
                style={{ transform: `scale(${zoom})` }}
              >
                <defs>
                  <marker
                    id="project-arrowhead"
                    viewBox="0 0 5 5"
                    refX="4.4"
                    refY="2.5"
                    markerWidth="4"
                    markerHeight="4"
                    orient="auto-start-reverse"
                  >
                    <path d="M0 0 5 2.5 0 5z" fill="var(--project-muted)" />
                  </marker>
                  <marker
                    id="project-arrowhead-accent"
                    viewBox="0 0 5 5"
                    refX="4.4"
                    refY="2.5"
                    markerWidth="4"
                    markerHeight="4"
                    orient="auto-start-reverse"
                  >
                    <path d="M0 0 5 2.5 0 5z" fill="var(--project-accent)" />
                  </marker>
                </defs>
                {postgresResource && (
                  <path
                    className={connectorClassName(
                      "project-postgres",
                      selectedNode
                    )}
                    data-connector="project-postgres"
                    d="M42 53 V46 H42 V44"
                    markerEnd={
                      selectedNode === "postgres"
                        ? "url(#project-arrowhead-accent)"
                        : "url(#project-arrowhead)"
                    }
                  />
                )}
              </svg>

              {resourcesLoading && (
                <div className="project-canvas-message" role="status">
                  <Spinner />
                  <span>Loading resources</span>
                </div>
              )}
              {!resourcesLoading && resourceError && (
                <div className="project-canvas-message project-canvas-message-error">
                  <strong>Resources unavailable</strong>
                  <span>{resourceError}</span>
                </div>
              )}
              {!resourcesLoading && !resourceError && !postgresResource && (
                <div className="project-canvas-empty">
                  <span
                    className="project-canvas-empty-icon"
                    aria-hidden="true"
                  >
                    <DatabaseIcon />
                  </span>
                  <strong>Create your first database</strong>
                  <span>
                    Add a real PostgreSQL database to give this project a
                    durable data store.
                  </span>
                  <Button
                    type="button"
                    size="sm"
                    onClick={() => setPostgresDialogOpen(true)}
                  >
                    Create database
                  </Button>
                </div>
              )}

              <div
                className="project-canvas-world"
                style={{ transform: `scale(${zoom})` }}
              >
                {nodes.map((node) => (
                  <article
                    key={node.id}
                    className={cn(
                      "project-node-card",
                      selectedNode === node.id && "is-selected"
                    )}
                    style={{
                      left: `${node.position.left}%`,
                      top: `${node.position.top}%`,
                    }}
                    tabIndex={0}
                    role="button"
                    aria-pressed={selectedNode === node.id}
                    aria-label={`${node.title} resource, ${node.status.toLowerCase()}`}
                    onClick={() => selectNode(node.id)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault()
                        selectNode(node.id)
                      }
                    }}
                  >
                    <div className="project-node-main">
                      <div className="project-node-title">
                        <span className="project-node-logo" aria-hidden="true">
                          <NodeIcon nodeId={node.id} />
                        </span>
                        <div>
                          <h2 className="project-node-heading">{node.title}</h2>
                          {node.subtitle && (
                            <p className="project-node-subtitle">
                              {node.subtitle}
                            </p>
                          )}
                        </div>
                      </div>
                      <div className="project-node-status">
                        <span
                          className={cn(
                            "project-status-dot",
                            `project-status-dot-${node.status.toLowerCase()}`
                          )}
                          aria-hidden="true"
                        />
                        <span>{node.status}</span>
                      </div>
                    </div>
                    <div className="project-node-footer">
                      <HardDriveIcon
                        className="project-storage-icon"
                        aria-hidden="true"
                      />
                      <span>{node.volume}</span>
                    </div>
                  </article>
                ))}
              </div>

              <div className="project-selection-hint" aria-hidden="true">
                <kbd>Click</kbd> a resource to inspect
              </div>

              <div className="project-zoom-dock" aria-label="Canvas controls">
                <div className="project-zoom-group">
                  <button
                    className="project-zoom-button"
                    type="button"
                    aria-label="Zoom in"
                    onClick={() => updateZoom(zoom + 0.1, "Zoomed in")}
                  >
                    <ZoomInIcon aria-hidden="true" />
                  </button>
                  <button
                    className="project-zoom-button"
                    type="button"
                    aria-label="Zoom out"
                    onClick={() => updateZoom(zoom - 0.1, "Zoomed out")}
                  >
                    <ZoomOutIcon aria-hidden="true" />
                  </button>
                </div>
                <div className="project-history-group">
                  <button
                    className="project-zoom-button"
                    type="button"
                    aria-label="Fit topology to view"
                    onClick={() => updateZoom(1, "View fitted")}
                  >
                    <Maximize2Icon aria-hidden="true" />
                  </button>
                  <button
                    className="project-zoom-button"
                    type="button"
                    aria-label="Undo view change"
                    onClick={() => showToast("No earlier view change")}
                  >
                    <Undo2Icon aria-hidden="true" />
                  </button>
                  <button
                    className="project-zoom-button"
                    type="button"
                    aria-label="Redo view change"
                    onClick={() => showToast("No later view change")}
                  >
                    <Redo2Icon aria-hidden="true" />
                  </button>
                </div>
                <button
                  className="project-layers-button"
                  type="button"
                  aria-label="Toggle layer guidance"
                  aria-pressed={layersVisible}
                  onClick={() => {
                    setLayersVisible((current) => !current)
                    showToast(
                      layersVisible
                        ? "Layer guidance hidden"
                        : "Layer guidance visible"
                    )
                  }}
                >
                  <Layers3Icon aria-hidden="true" />
                </button>
                <span className="project-zoom-readout" aria-live="polite">
                  {Math.round(zoom * 100)}%
                </span>
              </div>
            </section>
          </>
        )}
      </main>

      {selectedNodeData && (
        <ResourceWorkspace
          key={selectedNodeData.id}
          node={selectedNodeData}
          environment={environment}
          workspaceSlug={workspaceSlug}
          projectSlug={projectSlug}
          onClose={closeResourceWorkspace}
          onCopyConnectionString={(value) => {
            void handleCopyConnectionString(value)
          }}
          copiedConnectionString={copiedConnectionString}
          onToast={showToast}
          onOpenLogs={openResourceLogs}
          onAppServiceUpdated={(resource) => {
            setAppService(resource)
            setResourceError(null)
          }}
        />
      )}

      <PostgresCreateDialog
        key={
          postgresDialogOpen ? "postgres-dialog-open" : "postgres-dialog-closed"
        }
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        open={postgresDialogOpen}
        onOpenChange={setPostgresDialogOpen}
        onCreated={handlePostgresCreated}
      />

      <AppServiceCreateDialog
        key={
          appServiceDialogOpen
            ? "app-service-dialog-open"
            : "app-service-dialog-closed"
        }
        workspaceSlug={workspaceSlug}
        projectSlug={projectSlug}
        open={appServiceDialogOpen}
        onOpenChange={setAppServiceDialogOpen}
        onCreated={handleAppServiceCreated}
      />

      {toast && (
        <div className="project-toast" role="status" aria-live="polite">
          {toast}
        </div>
      )}
    </div>
  )
}

function ProjectRailButton({
  active = false,
  label,
  onClick,
  children,
}: {
  active?: boolean
  label: string
  onClick: () => void
  children: ReactNode
}) {
  return (
    <button
      className="project-rail-button"
      type="button"
      aria-current={active ? "page" : undefined}
      aria-label={label}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function LogsWorkspace({
  project,
  environment,
}: {
  project: Project
  environment: string
}) {
  const [resourceFilter, setResourceFilter] = useState("all")
  const [search, setSearch] = useState("")
  const [live, setLive] = useState(true)

  return (
    <section className="project-logs-shell" aria-label={`${project.name} logs`}>
      <div className="project-logs-toolbar">
        <div className="project-logs-heading">
          <h1>Logs</h1>
          <p>
            Runtime events for {project.name} · {environment}
          </p>
        </div>
        <div className="project-logs-filters">
          <label>
            <span className="sr-only">Filter logs by resource</span>
            <select
              value={resourceFilter}
              onChange={(event) => setResourceFilter(event.target.value)}
            >
              <option value="all">All resources</option>
              <option value="postgres">Postgres</option>
              <option value="project">App service</option>
            </select>
          </label>
          <label className="project-logs-search">
            <SearchIcon aria-hidden="true" />
            <span className="sr-only">Search logs</span>
            <input
              type="search"
              value={search}
              placeholder="Filter messages..."
              onChange={(event) => setSearch(event.target.value)}
            />
          </label>
          <button
            className="project-logs-live"
            type="button"
            aria-pressed={live}
            onClick={() => setLive((current) => !current)}
          >
            <span aria-hidden="true" />
            {live ? "Live" : "Paused"}
          </button>
        </div>
      </div>
      <div className="project-logs-stream" role="log" aria-live="polite">
        <div className="project-logs-empty">
          <span className="project-logs-empty-icon" aria-hidden="true">
            <FileTextIcon />
          </span>
          <h2>No logs yet</h2>
          <p>
            Logs will appear here once{" "}
            {resourceFilter === "all" ? "a resource" : "this resource"} starts
            handling traffic.
          </p>
          {(search || !live) && (
            <span className="project-logs-filter-note">
              {search ? `Filtering for “${search}” · ` : ""}
              {live ? "Live tail enabled" : "Live tail paused"}
            </span>
          )}
        </div>
      </div>
    </section>
  )
}

function ProjectAddOption({
  label,
  mark,
  onClick,
}: {
  label: string
  mark: string
  onClick: () => void
}) {
  return (
    <button className="project-menu-option" type="button" onClick={onClick}>
      <span className="project-menu-icon" aria-hidden="true">
        {mark}
      </span>
      <span>{label}</span>
    </button>
  )
}

function NodeIcon({ nodeId }: { nodeId: TopologyNodeId }) {
  if (nodeId === "postgres") {
    return <DatabaseIcon aria-hidden="true" />
  }
  return <GitBranchIcon aria-hidden="true" />
}

function connectorClassName(
  connector: ConnectorId,
  selectedNode: TopologyNodeId | null
) {
  const highlighted =
    connector === "project-postgres" && selectedNode === "postgres"
  return cn("project-connector", highlighted && "is-highlighted")
}

function resourceStatusLabel(status: PostgresResourceStatus | AppServiceStatus) {
  if (status === "ready") {
    return "Online"
  }
  if (status === "provisioning") {
    return "Creating"
  }
  return "Needs attention"
}

function readPersistedState(key: string): Required<PersistedDashboardState> {
  const fallback: Required<PersistedDashboardState> = {
    zoom: 1,
    selectedNode: null,
  }
  try {
    const saved = JSON.parse(
      window.localStorage.getItem(key) ?? "null"
    ) as PersistedDashboardState | null
    if (!saved) {
      return fallback
    }
    const zoom = typeof saved.zoom === "number" ? saved.zoom : fallback.zoom
    const selectedNode = isTopologyNodeId(saved.selectedNode)
      ? saved.selectedNode
      : null
    return {
      zoom: Math.min(1.25, Math.max(0.8, zoom)),
      selectedNode,
    }
  } catch {
    return fallback
  }
}

function isTopologyNodeId(value: unknown): value is TopologyNodeId {
  return value === "postgres" || value === "project"
}

function getInitial(value: string) {
  return value.trim().charAt(0).toUpperCase() || "K"
}

function readStoredTheme(key: string): Theme {
  try {
    return window.localStorage.getItem(key) === "dark" ? "dark" : "light"
  } catch {
    return "light"
  }
}
