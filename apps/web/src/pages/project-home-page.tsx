import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react"
import {
  ArrowUpRightIcon,
  BarChart3Icon,
  BoxIcon,
  DatabaseIcon,
  FileTextIcon,
  GitBranchIcon,
  HardDriveIcon,
  Maximize2Icon,
  NetworkIcon,
  SearchIcon,
  Settings2Icon,
  MoonIcon,
  SunIcon,
  XIcon,
  ZoomInIcon,
  ZoomOutIcon,
} from "lucide-react"
import { cn } from "cn"
import { useNavigate, useParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { AppServiceCreateDialog } from "@/components/app-service-create-dialog"
import { AppServiceDeploymentLogs } from "@/components/app-service-deployment-logs"
import { PostgresCreateDialog } from "@/components/postgres-create-dialog"
import { RedisCreateDialog } from "@/components/redis-create-dialog"
import { ResourceWorkspace } from "@/components/resource-workspace"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { getProject } from "@/lib/projects"
import {
  appServiceDeploymentEventsUrl,
  listAppServices,
  listPostgresResources,
  listRedisResources,
} from "@/lib/resources"
import {
  defaultPostgresPosition,
  defaultRedisPosition,
  defaultServicePosition,
  mergePositions,
  moveNodePosition,
  pointerDeltaPercent,
  type NodePosition,
} from "@/lib/topology-layout"
import type {
  AppService,
  AppServiceDeployment,
  AppServiceStatus,
  PostgresResource,
  PostgresResourceStatus,
  Project,
  RedisResource,
  RedisResourceStatus,
  Workspace,
} from "@/lib/types"

import "./project-home.css"

type TopologyNodeId = string
type ConnectorId = string
type Environment = "production" | "staging"
type Theme = "light" | "dark"
type WorkspaceView = "resources" | "topology" | "metrics" | "logs"

type TopologyNode = {
  id: TopologyNodeId
  title: string
  subtitle?: string
  type: string
  volume: string
  status: string
  resource?: PostgresResource | AppService | RedisResource
  position: { left: number; top: number }
}

type PersistedDashboardState = {
  zoom?: number
  selectedNode?: string | null
  positions?: Record<string, NodePosition>
}

export function ProjectHomePage() {
  const { session } = useAuth()
  const { workspaceId, projectSlug } = useParams()
  const [project, setProject] = useState<Project | null>(null)
  const [loadError, setLoadError] = useState<string | null>(null)

  useEffect(() => {
    if (!workspaceId || !projectSlug) {
      return undefined
    }

    let active = true
    void getProject(workspaceId, projectSlug)
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
  }, [projectSlug, workspaceId])

  if (!session?.workspace || !workspaceId || !projectSlug) {
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
      workspaceId={workspaceId}
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
      <Button onClick={() => navigate(`/workspace/${workspace.id}`)}>
        Back to workspace
      </Button>
    </main>
  )
}

function TopologyDashboard({
  project,
  workspace,
  workspaceId,
  projectSlug,
}: {
  project: Project
  workspace: Workspace
  workspaceId: string
  projectSlug: string
}) {
  const navigate = useNavigate()
  const { session, signOut } = useAuth()
  const [postgresResource, setPostgresResource] =
    useState<PostgresResource | null>(null)
  const [redisResource, setRedisResource] = useState<RedisResource | null>(null)
  const [appServices, setAppServices] = useState<AppService[]>([])
  const [resourcesLoading, setResourcesLoading] = useState(true)
  const [resourceError, setResourceError] = useState<string | null>(null)
  const [postgresDialogOpen, setPostgresDialogOpen] = useState(false)
  const [redisDialogOpen, setRedisDialogOpen] = useState(false)
  const [appServiceDialogOpen, setAppServiceDialogOpen] = useState(false)
  const [copiedConnectionString, setCopiedConnectionString] = useState(false)
  const [theme, setTheme] = useState<Theme>(() =>
    readStoredTheme("project-topology-dashboard-theme")
  )
  const [canvasTheme, setCanvasTheme] = useState<Theme>(() =>
    readStoredTheme("project-topology-canvas-theme")
  )
  const [activeView, setActiveView] = useState<WorkspaceView>("resources")
  const [resourceWorkspaceInitialTab, setResourceWorkspaceInitialTab] =
    useState<"deployments" | "metrics">("deployments")
  const canvasRef = useRef<HTMLDivElement>(null)
  const dragRef = useRef<{ id: string; moved: boolean } | null>(null)
  const stateKey = `project-topology-dashboard-state:${project.id}`
  const initialState = useMemo(() => readPersistedState(stateKey), [stateKey])
  const [positions, setPositions] = useState<Record<string, NodePosition>>(
    initialState.positions
  )
  const nodes = useMemo<TopologyNode[]>(() => {
    const defaults: Record<string, NodePosition> = {}
    const databaseNode: TopologyNode | null = postgresResource
      ? {
          id: "postgres",
          title: postgresResource.name,
          subtitle: postgresResource.databaseName,
          type: "PostgreSQL database",
          volume: postgresResource.databaseName,
          status: resourceStatusLabel(postgresResource.status),
          resource: postgresResource,
          position: defaultPostgresPosition(),
        }
      : null
    if (databaseNode) {
      defaults.postgres = databaseNode.position
    }
    const redisNode: TopologyNode | null = redisResource
      ? {
          id: "redis",
          title: redisResource.name,
          subtitle: redisResource.networkAlias,
          type: "Redis",
          volume: redisResource.clusterName ?? `${redisResource.name}-volume`,
          status: resourceStatusLabel(redisResource.status),
          resource: redisResource,
          position: defaultRedisPosition(Boolean(postgresResource)),
        }
      : null
    if (redisNode) {
      defaults.redis = redisNode.position
    }
    const serviceNodes = appServices.map((service, index) => {
      const id = appServiceNodeId(service.id)
      const position = defaultServicePosition(
        index,
        appServices.length,
        Boolean(postgresResource || redisResource)
      )
      defaults[id] = position
      return {
        id,
        title: appServices.length === 1 ? project.name : service.name,
        subtitle:
          appServices.length === 1
            ? `${service.name} · ${
                service.imageSource === "html" ||
                service.imageSource === "html_github"
                  ? (service.publicDomain ?? "HTML page")
                  : service.image
              }`
            : service.imageSource === "html" ||
                service.imageSource === "html_github"
              ? (service.publicDomain ?? "HTML page")
              : service.image,
        type:
          service.imageSource === "html" ||
          service.imageSource === "html_github"
            ? "HTML page"
            : "App service",
        volume: service.containerName ?? `${service.name}-volume`,
        status: resourceStatusLabel(service.status),
        resource: service,
        position,
      }
    })
    const merged = mergePositions(defaults, positions)
    const placed = [databaseNode, redisNode, ...serviceNodes].filter(
      (node): node is TopologyNode => Boolean(node)
    )
    if (placed.length > 0) {
      return placed.map((node) => ({
        ...node,
        position: merged[node.id] ?? node.position,
      }))
    }
    const emptyServiceNode: TopologyNode = {
      id: "project",
      title: project.name,
      subtitle: project.slug,
      type: "App service",
      volume: `${project.slug}-volume`,
      status: "Needs setup",
      position: merged.project ?? { left: 50, top: 42 },
    }
    return [emptyServiceNode]
  }, [
    appServices,
    positions,
    postgresResource,
    project.name,
    project.slug,
    redisResource,
  ])
  const [zoom, setZoom] = useState(initialState.zoom)
  const [selectedNode, setSelectedNode] = useState<TopologyNodeId | null>(
    initialState.selectedNode
  )
  const environment: Environment = "production"
  const [addMenuOpen, setAddMenuOpen] = useState(false)
  const [toast, setToast] = useState<string | null>(null)
  const toastTimer = useRef<number | null>(null)
  const copyTimer = useRef<number | null>(null)
  const addMenuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    let active = true
    void Promise.all([
      listPostgresResources(workspaceId, projectSlug),
      listRedisResources(workspaceId, projectSlug),
      listAppServices(workspaceId, projectSlug),
    ])
      .then(([postgresResources, redisResources, appServices]) => {
        if (!active) {
          return
        }
        setPostgresResource(postgresResources[0] ?? null)
        setRedisResource(redisResources[0] ?? null)
        setAppServices(appServices)
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
  }, [projectSlug, workspaceId])

  const appServiceIsProvisioning = appServices.some(
    (service) => service.status === "provisioning"
  )

  const postgresIsProvisioning = postgresResource?.status === "provisioning"

  useEffect(() => {
    if (!postgresIsProvisioning) {
      return undefined
    }

    let active = true
    const refresh = () => {
      void listPostgresResources(workspaceId, projectSlug)
        .then((resources) => {
          if (active) {
            setPostgresResource(resources[0] ?? null)
            setResourceError(null)
          }
        })
        .catch(() => {
          // Keep the last resource snapshot visible if a background refresh
          // is temporarily unavailable.
        })
    }
    const intervalId = window.setInterval(refresh, 2000)
    return () => {
      active = false
      window.clearInterval(intervalId)
    }
  }, [postgresIsProvisioning, projectSlug, workspaceId])

  useEffect(() => {
    if (!appServiceIsProvisioning) {
      return undefined
    }

    let active = true
    const refresh = () => {
      void listAppServices(workspaceId, projectSlug)
        .then((resources) => {
          if (active) {
            setAppServices(resources)
          }
        })
        .catch(() => {
          // The deployment stream owns detailed errors; keep the last resource
          // snapshot visible if a background refresh is temporarily unavailable.
        })
    }
    const intervalId = window.setInterval(refresh, 2000)
    return () => {
      active = false
      window.clearInterval(intervalId)
    }
  }, [appServiceIsProvisioning, projectSlug, workspaceId])

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
      JSON.stringify({
        zoom,
        selectedNode,
        positions,
      } satisfies PersistedDashboardState)
    )
  }, [positions, selectedNode, stateKey, zoom])

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
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") {
        return
      }
      setAddMenuOpen(false)
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
    setResourceWorkspaceInitialTab("deployments")
    setSelectedNode(nodeId)
  }

  function handlePostgresAdd() {
    setAddMenuOpen(false)
    if (postgresResource?.status === "ready") {
      selectNode("postgres")
      showToast("Postgres is already provisioned")
      return
    }
    setPostgresDialogOpen(true)
  }

  function handlePostgresCreated(resource: PostgresResource) {
    setPostgresResource(resource)
    setResourceError(null)
    setPostgresDialogOpen(false)
    selectNode("postgres")
    showToast(
      resource.status === "provisioning"
        ? "Postgres database creation started"
        : "Postgres database is ready"
    )
  }

  function handleRedisAdd() {
    setAddMenuOpen(false)
    if (redisResource?.status === "ready") {
      selectNode("redis")
      showToast("Redis is already provisioned")
      return
    }
    setRedisDialogOpen(true)
  }

  function handleRedisCreated(resource: RedisResource) {
    setRedisResource(resource)
    setResourceError(null)
    setRedisDialogOpen(false)
    selectNode("redis")
    showToast("Redis is ready")
  }

  function handleNodePointerDown(
    nodeId: TopologyNodeId,
    event: ReactPointerEvent<HTMLElement>
  ) {
    if (typeof event.currentTarget.setPointerCapture === "function") {
      event.currentTarget.setPointerCapture(event.pointerId)
    }
    dragRef.current = { id: nodeId, moved: false }
  }

  function handleNodePointerMove(event: ReactPointerEvent<HTMLElement>) {
    const drag = dragRef.current
    if (!drag || event.buttons === 0) {
      return
    }
    const bounds = canvasRef.current?.getBoundingClientRect()
    if (!bounds) {
      return
    }
    const delta = pointerDeltaPercent(
      event.movementX,
      event.movementY,
      bounds.width,
      bounds.height
    )
    if (delta.left === 0 && delta.top === 0) {
      return
    }
    drag.moved = true
    setPositions((current) => {
      const existing = current[drag.id] ??
        nodes.find((node) => node.id === drag.id)?.position ?? {
          left: 50,
          top: 40,
        }
      return {
        ...current,
        [drag.id]: moveNodePosition(existing, delta),
      }
    })
  }

  function handleNodePointerUp(nodeId: TopologyNodeId) {
    if (!dragRef.current?.moved) {
      selectNode(nodeId)
    }
    dragRef.current = null
  }

  function handleAppServiceAdd() {
    setAddMenuOpen(false)
    if (appServices.length >= 6) {
      showToast("A project can have up to 6 app services")
      return
    }
    setAppServiceDialogOpen(true)
  }

  function handleAppServiceCreated(resource: AppService) {
    setAppServices((current) => {
      const existing = current.some((service) => service.id === resource.id)
      return existing
        ? current.map((service) =>
            service.id === resource.id ? resource : service
          )
        : [...current, resource]
    })
    setResourceError(null)
    setResourceWorkspaceInitialTab("deployments")
    setSelectedNode(appServiceNodeId(resource.id))
    showToast(
      resource.status === "provisioning"
        ? "App service deployment started"
        : "App service is deployed"
    )
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
            aria-label={`Open ${workspace.name} workspace`}
            onClick={() => navigate(`/workspace/${workspace.id}`)}
          >
            <span className="project-brand-mark" aria-hidden="true">
              <span />
            </span>
          </button>
          <span className="project-topbar-divider" aria-hidden="true" />
          <div className="project-workspace-switcher">
            <span className="project-avatar-dot" aria-hidden="true">
              {getInitial(workspace.name)}
            </span>
            <button
              className="project-context-trigger"
              type="button"
              aria-label={`Return to ${workspace.name} workspace`}
              onClick={() => navigate(`/workspace/${workspace.id}`)}
            >
              <span className="project-context-label">{workspace.name}</span>
            </button>
          </div>
          <span className="project-topbar-divider" aria-hidden="true" />
          <span className="project-project-context">{project.name}</span>
          <span className="project-environment-context">{environment}</span>
        </div>

        <div className="project-topbar-right">
          <div ref={addMenuRef} className="project-add-wrap">
            <button
              className="project-primary-button"
              type="button"
              aria-expanded={addMenuOpen}
              aria-controls="project-add-menu"
              onClick={() => setAddMenuOpen((current) => !current)}
            >
              <span className="project-plus" aria-hidden="true">
                +
              </span>
              <span className="project-add-label">Add resource</span>
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
                    : postgresResource?.status === "provisioning"
                      ? "Postgres (creating)"
                      : "Postgres"
                }
                onClick={handlePostgresAdd}
              />
              <ProjectAddOption
                mark="R"
                label={
                  redisResource?.status === "ready" ? "Redis (ready)" : "Redis"
                }
                onClick={handleRedisAdd}
              />
              <ProjectAddOption
                mark="S"
                label="App service / HTML page"
                disabled={appServices.length >= 6}
                onClick={handleAppServiceAdd}
              />
            </div>
          </div>
          <div className="project-topbar-actions" aria-label="Account actions">
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
          </div>
          <button
            className="project-agent-button"
            type="button"
            aria-label={`Sign out ${session?.user.email ?? "account"}`}
            onClick={() => void handleSignOut()}
          >
            <span className="project-agent-avatar" aria-hidden="true">
              {memberInitial}
            </span>
            <span className="project-sign-out-label">Sign out</span>
          </button>
        </div>
      </header>

      <aside className="project-side-rail">
        <div className="project-rail-project">
          <strong>{project.name}</strong>
          <span>{workspace.name}</span>
        </div>
        <nav className="project-rail-nav" aria-label="Primary">
          <ProjectRailButton
            active={activeView === "resources"}
            label="Resources"
            onClick={() => setActiveView("resources")}
          >
            <BoxIcon aria-hidden="true" />
          </ProjectRailButton>
          <ProjectRailButton
            active={activeView === "topology"}
            label="Topology"
            onClick={() => setActiveView("topology")}
          >
            <NetworkIcon aria-hidden="true" />
          </ProjectRailButton>
          <ProjectRailButton
            active={activeView === "metrics"}
            label="Metrics"
            onClick={() => setActiveView("metrics")}
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
            label="Integrations"
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
          <LogsWorkspace
            project={project}
            environment={environment}
            appServices={appServices}
            workspaceId={workspaceId}
            projectSlug={projectSlug}
          />
        ) : activeView === "resources" ? (
          <ResourcesWorkspace
            project={project}
            nodes={nodes.filter((node) => Boolean(node.resource))}
            loading={resourcesLoading}
            error={resourceError}
            onOpenResource={(node) => selectNode(node.id)}
            onAddResource={() => setAddMenuOpen(true)}
          />
        ) : activeView === "metrics" ? (
          <MetricsWorkspace
            project={project}
            nodes={nodes.filter((node) => Boolean(node.resource))}
            loading={resourcesLoading}
            error={resourceError}
            onOpenMetrics={(node) => {
              setResourceWorkspaceInitialTab("metrics")
              setSelectedNode(node.id)
            }}
          />
        ) : (
          <>
            <h1 className="sr-only">{project.name} infrastructure topology</h1>
            <section
              className={cn(
                "project-topology-shell",
                selectedNode && "has-selection"
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
                {postgresResource &&
                  appServices.map((service) => {
                    if (!service.databaseConnection) {
                      return null
                    }
                    const serviceNode = nodes.find(
                      (node) => node.id === appServiceNodeId(service.id)
                    )
                    if (!serviceNode) {
                      return null
                    }
                    const connector = appServiceDatabaseConnectorId(service.id)
                    const left = serviceNode.position.left
                    const top = serviceNode.position.top
                    return (
                      <path
                        key={connector}
                        className={connectorClassName(connector, selectedNode)}
                        data-connector={connector}
                        d={`M50 27 V32 H${left} V${top}`}
                        markerEnd={
                          selectedNode === serviceNode.id
                            ? "url(#project-arrowhead-accent)"
                            : "url(#project-arrowhead)"
                        }
                      />
                    )
                  })}
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
                ref={canvasRef}
                className="project-canvas-world"
                style={{ transform: `scale(${zoom})` }}
              >
                {nodes.map((node) => (
                  <article
                    key={node.id}
                    className={cn(
                      "project-node-card",
                      node.resource?.resourceType === "app" &&
                        "project-app-node",
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
                    onPointerDown={(event) =>
                      handleNodePointerDown(node.id, event)
                    }
                    onPointerMove={handleNodePointerMove}
                    onPointerUp={() => handleNodePointerUp(node.id)}
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
                <button
                  className="project-zoom-button"
                  type="button"
                  aria-label="Fit topology to view"
                  onClick={() => updateZoom(1, "View fitted")}
                >
                  <Maximize2Icon aria-hidden="true" />
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
          key={`${selectedNodeData.id}:${resourceWorkspaceInitialTab}`}
          node={selectedNodeData}
          environment={environment}
          workspaceId={workspaceId}
          projectSlug={projectSlug}
          initialTab={resourceWorkspaceInitialTab}
          onClose={closeResourceWorkspace}
          onCopyConnectionString={(value) => {
            void handleCopyConnectionString(value)
          }}
          copiedConnectionString={copiedConnectionString}
          onToast={showToast}
          onOpenLogs={openResourceLogs}
          onAppServiceUpdated={(resource) => {
            setAppServices((current) =>
              current.map((service) =>
                service.id === resource.id ? resource : service
              )
            )
            setResourceError(null)
          }}
          onPostgresUpdated={(resource) => {
            setPostgresResource(resource)
            setResourceError(null)
          }}
        />
      )}

      <PostgresCreateDialog
        key={
          postgresDialogOpen ? "postgres-dialog-open" : "postgres-dialog-closed"
        }
        workspaceId={workspaceId}
        projectSlug={projectSlug}
        open={postgresDialogOpen}
        onOpenChange={setPostgresDialogOpen}
        onCreated={handlePostgresCreated}
      />

      <RedisCreateDialog
        key={redisDialogOpen ? "redis-dialog-open" : "redis-dialog-closed"}
        workspaceId={workspaceId}
        projectSlug={projectSlug}
        open={redisDialogOpen}
        onOpenChange={setRedisDialogOpen}
        onCreated={handleRedisCreated}
      />

      <AppServiceCreateDialog
        key={
          appServiceDialogOpen
            ? "app-service-dialog-open"
            : "app-service-dialog-closed"
        }
        workspaceId={workspaceId}
        projectSlug={projectSlug}
        open={appServiceDialogOpen}
        onOpenChange={setAppServiceDialogOpen}
        appServiceCount={appServices.length}
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
      <span className="project-rail-icon" aria-hidden="true">
        {children}
      </span>
      <span className="project-rail-label">{label}</span>
    </button>
  )
}

function ResourcesWorkspace({
  project,
  nodes,
  loading,
  error,
  onOpenResource,
  onAddResource,
}: {
  project: Project
  nodes: TopologyNode[]
  loading: boolean
  error: string | null
  onOpenResource: (node: TopologyNode) => void
  onAddResource: () => void
}) {
  const [search, setSearch] = useState("")
  const [filter, setFilter] = useState<"all" | "services" | "data">("all")
  const normalizedSearch = search.trim().toLowerCase()
  const filteredNodes = nodes.filter((node) => {
    const isDataStore =
      node.resource?.resourceType === "postgres" ||
      node.resource?.resourceType === "redis"
    const matchesFilter =
      filter === "all" || (filter === "data" ? isDataStore : !isDataStore)
    const matchesSearch =
      !normalizedSearch ||
      `${node.title} ${node.type} ${node.subtitle ?? ""}`
        .toLowerCase()
        .includes(normalizedSearch)
    return matchesFilter && matchesSearch
  })

  return (
    <section className="project-content-view" aria-labelledby="resources-title">
      <div className="project-view-heading">
        <div>
          <h1 id="resources-title">Resources</h1>
          <p>Manage app services and data stores for this project.</p>
        </div>
      </div>

      <div className="project-resource-toolbar">
        <label className="project-resource-search">
          <SearchIcon aria-hidden="true" />
          <span className="sr-only">Search resources</span>
          <input
            type="search"
            value={search}
            placeholder="Search resources"
            onChange={(event) => setSearch(event.target.value)}
          />
        </label>
        <div
          className="project-resource-filters"
          role="group"
          aria-label="Filter resources"
        >
          {(
            [
              ["all", "All resources"],
              ["services", "App services"],
              ["data", "Data stores"],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              type="button"
              aria-pressed={filter === value}
              className={cn(filter === value && "is-active")}
              onClick={() => setFilter(value)}
            >
              {label}
            </button>
          ))}
        </div>
      </div>

      {error && (
        <div className="project-view-error" role="alert">
          <strong>Resources unavailable</strong>
          <span>{error}</span>
        </div>
      )}

      {loading ? (
        <div className="project-view-loading" role="status">
          <Spinner />
          <span>Loading resources</span>
        </div>
      ) : nodes.length === 0 && !error ? (
        <div className="project-view-empty">
          <span className="project-view-empty-icon" aria-hidden="true">
            <BoxIcon />
          </span>
          <h2>No resources yet</h2>
          <p>
            Add an app service, HTML page, PostgreSQL database, or Redis store
            to get started.
          </p>
          <Button type="button" onClick={onAddResource}>
            Add resource
          </Button>
        </div>
      ) : filteredNodes.length === 0 && !error ? (
        <div className="project-view-empty project-view-empty-compact">
          <h2>No matching resources</h2>
          <p>Try another search or filter for {project.name}.</p>
        </div>
      ) : (
        <div className="project-resource-table-wrap">
          <table className="project-resource-table">
            <caption className="sr-only">Resources for {project.name}</caption>
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Type</th>
                <th scope="col">Status</th>
                <th scope="col">Open</th>
              </tr>
            </thead>
            <tbody>
              {filteredNodes.map((node) => (
                <tr key={node.id}>
                  <th scope="row" className="project-resource-name-cell">
                    <span className="project-resource-icon" aria-hidden="true">
                      <ResourceIcon node={node} />
                    </span>
                    <span className="project-resource-name">
                      <strong>{node.title}</strong>
                      <span className="project-resource-mobile-type">
                        {node.type}
                      </span>
                    </span>
                  </th>
                  <td className="project-resource-type-cell">{node.type}</td>
                  <td>
                    <span className="project-resource-status">
                      <span
                        className={cn(
                          "project-resource-status-dot",
                          `status-${node.status.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`
                        )}
                        aria-hidden="true"
                      />
                      {node.status}
                    </span>
                  </td>
                  <td className="project-resource-action-cell">
                    <button
                      type="button"
                      className="project-resource-open"
                      aria-label={`Open ${node.title}`}
                      onClick={() => onOpenResource(node)}
                    >
                      <span>Open</span>
                      <ArrowUpRightIcon aria-hidden="true" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function MetricsWorkspace({
  project,
  nodes,
  loading,
  error,
  onOpenMetrics,
}: {
  project: Project
  nodes: TopologyNode[]
  loading: boolean
  error: string | null
  onOpenMetrics: (node: TopologyNode) => void
}) {
  const metricNodes = nodes.filter(
    (node) =>
      node.resource?.resourceType === "postgres" ||
      node.resource?.resourceType === "app"
  )

  return (
    <section className="project-content-view" aria-labelledby="metrics-title">
      <div className="project-view-heading">
        <div>
          <h1 id="metrics-title">Metrics</h1>
          <p>
            Choose a PostgreSQL database or app service to view its live
            metrics.
          </p>
        </div>
      </div>

      {error && (
        <div className="project-view-error" role="alert">
          <strong>Resources unavailable</strong>
          <span>{error}</span>
        </div>
      )}

      {loading ? (
        <div className="project-view-loading" role="status">
          <Spinner />
          <span>Loading resources</span>
        </div>
      ) : metricNodes.length === 0 && !error ? (
        <div className="project-view-empty">
          <span className="project-view-empty-icon" aria-hidden="true">
            <BarChart3Icon />
          </span>
          <h2>No metrics resources yet</h2>
          <p>
            Add a PostgreSQL database or app service to view runtime metrics for
            {` ${project.name}`}.
          </p>
        </div>
      ) : (
        <div className="project-resource-table-wrap">
          <table className="project-resource-table project-metrics-table">
            <caption className="sr-only">
              Resources with runtime metrics in {project.name}
            </caption>
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Type</th>
                <th scope="col">Status</th>
                <th scope="col">Metrics</th>
              </tr>
            </thead>
            <tbody>
              {metricNodes.map((node) => (
                <tr key={node.id}>
                  <th scope="row" className="project-resource-name-cell">
                    <span className="project-resource-icon" aria-hidden="true">
                      <ResourceIcon node={node} />
                    </span>
                    <span className="project-resource-name">
                      <strong>{node.title}</strong>
                      <span className="project-resource-mobile-type">
                        {node.type}
                      </span>
                    </span>
                  </th>
                  <td className="project-resource-type-cell">{node.type}</td>
                  <td>
                    <span className="project-resource-status">
                      <span
                        className={cn(
                          "project-resource-status-dot",
                          `status-${node.status.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`
                        )}
                        aria-hidden="true"
                      />
                      {node.status}
                    </span>
                  </td>
                  <td className="project-resource-action-cell">
                    <button
                      type="button"
                      className="project-resource-open"
                      aria-label={`View metrics for ${node.title}`}
                      onClick={() => onOpenMetrics(node)}
                    >
                      <span>View metrics</span>
                      <ArrowUpRightIcon aria-hidden="true" />
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  )
}

function ResourceIcon({ node }: { node: TopologyNode }) {
  if (node.type === "HTML page") {
    return <FileTextIcon aria-hidden="true" />
  }
  if (node.resource?.resourceType === "app") {
    return <BoxIcon aria-hidden="true" />
  }
  return <NodeIcon nodeId={node.id} />
}

function LogsWorkspace({
  project,
  environment,
  appServices,
  workspaceId,
  projectSlug,
}: {
  project: Project
  environment: string
  appServices: AppService[]
  workspaceId: string
  projectSlug: string
}) {
  const [resourceFilter, setResourceFilter] = useState("all")
  const [search, setSearch] = useState("")
  const [live, setLive] = useState(true)
  const [liveDeployments, setLiveDeployments] = useState<
    Record<string, AppServiceDeployment>
  >({})

  useEffect(() => {
    if (!live || typeof EventSource === "undefined") {
      return undefined
    }
    const sources = appServices.flatMap((service) => {
      const deployment = service.deployment
      if (!deployment || deployment.status !== "provisioning") {
        return []
      }
      const source = new EventSource(
        appServiceDeploymentEventsUrl(
          workspaceId,
          projectSlug,
          deployment.id
        ),
        { withCredentials: true }
      )
      source.addEventListener("deployment", (event) => {
        try {
          const nextDeployment = JSON.parse(
            (event as MessageEvent<string>).data
          ) as AppServiceDeployment
          setLiveDeployments((current) => ({
            ...current,
            [service.id]: nextDeployment,
          }))
          if (nextDeployment.status !== "provisioning") {
            source.close()
          }
        } catch {
          // Keep the saved deployment snapshot when an event cannot be decoded.
        }
      })
      source.onerror = () => source.close()
      return [source]
    })
    return () => sources.forEach((source) => source.close())
  }, [appServices, live, projectSlug, workspaceId])

  const normalizedSearch = search.trim().toLowerCase()
  const showAppServiceLogs =
    resourceFilter === "all" || resourceFilter === "project"
  const visibleDeployments = showAppServiceLogs
    ? appServices.flatMap((service) => {
        const deployment = liveDeployments[service.id] ?? service.deployment
        if (!deployment) {
          return []
        }
        return [
          {
            service,
            deployment: {
              ...deployment,
              logs: deployment.logs.filter((line) =>
                line.toLowerCase().includes(normalizedSearch)
              ),
            },
          },
        ]
      })
    : []

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
        {visibleDeployments.length > 0 ? (
          visibleDeployments.map(({ service, deployment }) => (
            <div key={service.id} className="project-logs-deployment">
              <strong>{service.name}</strong>
              <AppServiceDeploymentLogs deployment={deployment} compact />
              {!live && (
                <span className="project-logs-filter-note">
                  Live tail paused; showing the last saved deployment snapshot.
                </span>
              )}
            </div>
          ))
        ) : (
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
        )}
      </div>
    </section>
  )
}

function ProjectAddOption({
  label,
  mark,
  disabled = false,
  onClick,
}: {
  label: string
  mark: string
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      className="project-menu-option"
      type="button"
      disabled={disabled}
      onClick={onClick}
    >
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
  if (nodeId === "redis") {
    return <HardDriveIcon aria-hidden="true" />
  }
  return <GitBranchIcon aria-hidden="true" />
}

function appServiceNodeId(serviceId: string) {
  return `app:${serviceId}`
}

function appServiceDatabaseConnectorId(serviceId: string) {
  return `database:app:${serviceId}`
}

function connectorClassName(
  connector: ConnectorId,
  selectedNode: TopologyNodeId | null
) {
  const highlighted = selectedNode !== null && connector.includes(selectedNode)
  return cn("project-connector", highlighted && "is-highlighted")
}

function resourceStatusLabel(
  status: PostgresResourceStatus | AppServiceStatus | RedisResourceStatus
) {
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
    positions: {},
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
      positions:
        saved.positions && typeof saved.positions === "object"
          ? saved.positions
          : fallback.positions,
    }
  } catch {
    return fallback
  }
}

function isTopologyNodeId(value: unknown): value is TopologyNodeId {
  return (
    value === "postgres" ||
    value === "redis" ||
    value === "project" ||
    (typeof value === "string" && value.startsWith("app:") && value.length > 4)
  )
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
