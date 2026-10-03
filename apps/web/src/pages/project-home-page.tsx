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
  ActivityIcon,
  ArrowUpRightIcon,
  BoxIcon,
  CheckIcon,
  ChevronRightIcon,
  DatabaseIcon,
  FileCodeIcon,
  HardDriveIcon,
  ImageIcon,
  LayersIcon,
  LayoutGridIcon,
  Maximize2Icon,
  MinusIcon,
  NetworkIcon,
  PlugIcon,
  PlusIcon,
  ScrollTextIcon,
  SearchIcon,
  XIcon,
} from "lucide-react"
import { cn } from "cn"
import { Link, useNavigate, useParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import {
  AppShell,
  BreadcrumbSeparator,
  SidebarNavItem,
} from "@/components/app-shell"
import { AppServiceCreateDialog } from "@/components/app-service-create-dialog"
import { ImageStoreCreateDialog } from "@/components/image-store-create-dialog"
import { ImageStoreWorkspace } from "@/components/image-store-workspace"
import { AppServiceDeploymentLogs } from "@/components/app-service-deployment-logs"
import { PostgresCreateDialog } from "@/components/postgres-create-dialog"
import { RedisCreateDialog } from "@/components/redis-create-dialog"
import { ResourceWorkspace } from "@/components/resource-workspace"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { initials } from "@/lib/initials"
import { getProject } from "@/lib/projects"
import {
  appServiceDeploymentEventsUrl,
  listAppServices,
  listImageStores,
  listPostgresResources,
  listRedisResources,
} from "@/lib/resources"
import {
  defaultImagePosition,
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
  ImageStore,
  PostgresResource,
  PostgresResourceStatus,
  Project,
  RedisResource,
  User,
  RedisResourceStatus,
  Workspace,
} from "@/lib/types"

import "./project-home.css"

type TopologyNodeId = string
type ConnectorId = string
type Environment = "production" | "staging"
type WorkspaceView = "resources" | "topology" | "metrics" | "logs"

type TopologyNode = {
  id: TopologyNodeId
  title: string
  subtitle?: string
  type: string
  volume: string
  status: string
  resource?: PostgresResource | AppService | RedisResource | ImageStore
  position: { left: number; top: number }
}

function isInfrastructureNode(node: TopologyNode): node is TopologyNode & {
  resource?: PostgresResource | AppService | RedisResource
} {
  return node.resource?.resourceType !== "images"
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
      user={session.user}
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
      <span className="empty-icon" aria-hidden="true">
        <XIcon />
      </span>
      <div className="flex max-w-md flex-col gap-2 text-center">
        <h1 className="text-xl font-semibold tracking-[-0.02em]">
          Project unavailable
        </h1>
        <p className="text-sm leading-6 text-muted-foreground">{message}</p>
      </div>
      <Button
        variant="outline"
        onClick={() => navigate(`/workspace/${workspace.id}`)}
      >
        Back to workspace
      </Button>
    </main>
  )
}

function TopologyDashboard({
  project,
  user,
  workspace,
  workspaceId,
  projectSlug,
}: {
  project: Project
  user: User
  workspace: Workspace
  workspaceId: string
  projectSlug: string
}) {
  const [postgresResource, setPostgresResource] =
    useState<PostgresResource | null>(null)
  const [redisResource, setRedisResource] = useState<RedisResource | null>(null)
  const [imageStores, setImageStores] = useState<ImageStore[]>([])
  const [appServices, setAppServices] = useState<AppService[]>([])
  const [resourcesLoading, setResourcesLoading] = useState(true)
  const [resourceError, setResourceError] = useState<string | null>(null)
  const [postgresDialogOpen, setPostgresDialogOpen] = useState(false)
  const [redisDialogOpen, setRedisDialogOpen] = useState(false)
  const [appServiceDialogOpen, setAppServiceDialogOpen] = useState(false)
  const [imageDialogOpen, setImageDialogOpen] = useState(false)
  const [copiedConnectionString, setCopiedConnectionString] = useState(false)
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
    const imageNodes = imageStores.map((store, index) => {
      const id = imageStoreNodeId(store.id)
      const position = defaultImagePosition(index)
      defaults[id] = position
      return {
        id,
        title: store.name,
        subtitle: store.compressionMode,
        type: "Image store",
        volume: store.publicBaseUrl,
        status: "Ready",
        resource: store,
        position,
      }
    })
    const serviceNodes = appServices.map((service, index) => {
      const id = appServiceNodeId(service.id)
      const position = defaultServicePosition(
        index,
        appServices.length,
        Boolean(postgresResource || redisResource || imageStores.length > 0)
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
    const placed = [
      databaseNode,
      redisNode,
      ...imageNodes,
      ...serviceNodes,
    ].filter((node): node is TopologyNode => Boolean(node))
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
    imageStores,
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
      listImageStores(workspaceId, projectSlug),
    ])
      .then(([postgresResources, redisResources, appServices, imageStores]) => {
        if (!active) {
          return
        }
        setPostgresResource(postgresResources[0] ?? null)
        setRedisResource(redisResources[0] ?? null)
        setAppServices(appServices)
        setImageStores(imageStores)
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

  function handleImagesAdd() {
    setAddMenuOpen(false)
    setImageDialogOpen(true)
  }

  function handleImageStoreCreated(store: ImageStore) {
    setImageStores((current) => [...current, store])
    setResourceError(null)
    setImageDialogOpen(false)
    selectNode(imageStoreNodeId(store.id))
  }

  async function refreshImageStores() {
    const stores = await listImageStores(workspaceId, projectSlug)
    setImageStores(stores)
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

  const selectedNodeData = nodes.find((node) => node.id === selectedNode)
  const resourceNodes = nodes.filter((node) => Boolean(node.resource))
  const addActions: AddResourceActions = {
    postgres: handlePostgresAdd,
    redis: handleRedisAdd,
    images: handleImagesAdd,
    app: handleAppServiceAdd,
  }

  return (
    <AppShell
      className="project-home"
      workspace={workspace}
      user={user}
      context={
        <>
          <span className="app-sidebar-label">Project</span>
          <div className="app-sidebar-project">
            <span className="project-initial" aria-hidden="true">
              {initials(project.name)}
            </span>
            <span>
              <strong>{project.name}</strong>
              <small>/{project.slug}</small>
            </span>
          </div>
        </>
      }
      nav={
        <>
          <SidebarNavItem
            active={activeView === "resources"}
            label="Resources"
            icon={<LayoutGridIcon />}
            onClick={() => setActiveView("resources")}
          />
          <SidebarNavItem
            active={activeView === "topology"}
            label="Topology"
            icon={<NetworkIcon />}
            onClick={() => setActiveView("topology")}
          />
          <SidebarNavItem
            active={activeView === "metrics"}
            label="Metrics"
            icon={<ActivityIcon />}
            onClick={() => setActiveView("metrics")}
          />
          <SidebarNavItem
            active={activeView === "logs"}
            label="Logs"
            icon={<ScrollTextIcon />}
            onClick={() => {
              setSelectedNode(null)
              setActiveView("logs")
            }}
          />
        </>
      }
      footerNav={
        <SidebarNavItem
          label="Integrations"
          icon={<PlugIcon />}
          to="/settings/integrations"
        />
      }
      breadcrumbs={
        <>
          <Link
            to={`/workspace/${workspace.id}`}
            className="app-breadcrumb-link app-breadcrumb-hide-mobile"
            aria-label={`Return to ${workspace.name} workspace`}
          >
            {workspace.name}
          </Link>
          <span className="app-breadcrumb-hide-mobile">
            <BreadcrumbSeparator />
          </span>
          <span className="app-breadcrumb-current project-project-context">
            {project.name}
          </span>
          <span className="app-env-badge">{environment}</span>
        </>
      }
      actions={
        <div ref={addMenuRef} className="project-add-wrap">
          <Button
            size="sm"
            aria-expanded={addMenuOpen}
            aria-controls="project-add-menu"
            aria-label="Add resource"
            onClick={() => setAddMenuOpen((current) => !current)}
          >
            <PlusIcon data-icon="inline-start" />
            <span className="project-add-label">Add resource</span>
          </Button>
          <div
            id="project-add-menu"
            className="app-menu project-add-menu"
            hidden={!addMenuOpen}
          >
            <div className="project-menu-heading">Add to {project.name}</div>
            <ProjectAddOption
              icon={<DatabaseIcon />}
              label={
                postgresResource?.status === "ready"
                  ? "Postgres (ready)"
                  : postgresResource?.status === "provisioning"
                    ? "Postgres (creating)"
                    : "Postgres"
              }
              description="Dedicated relational database"
              onClick={handlePostgresAdd}
            />
            <ProjectAddOption
              icon={<LayersIcon />}
              label={
                redisResource?.status === "ready" ? "Redis (ready)" : "Redis"
              }
              description="In-memory cache and queues"
              onClick={handleRedisAdd}
            />
            <ProjectAddOption
              icon={<ImageIcon />}
              label="Images"
              description="Image storage with resizing"
              onClick={handleImagesAdd}
            />
            <ProjectAddOption
              icon={<BoxIcon />}
              label="App service / HTML page"
              description={
                appServices.length >= 6
                  ? "Limit of 6 services reached"
                  : "Deploy a container or static site"
              }
              disabled={appServices.length >= 6}
              onClick={handleAppServiceAdd}
            />
          </div>
        </div>
      }
    >
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
            nodes={resourceNodes}
            loading={resourcesLoading}
            error={resourceError}
            onOpenResource={(node) => selectNode(node.id)}
            addActions={addActions}
          />
        ) : activeView === "metrics" ? (
          <MetricsWorkspace
            project={project}
            nodes={resourceNodes}
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
            >
              <div className="project-canvas-legend" aria-hidden="true">
                <span>
                  <i className="is-service" /> Services
                </span>
                <span>
                  <i className="is-data" /> Data stores
                </span>
                <span>
                  <i className="is-link" /> Private network
                </span>
              </div>

              <svg
                className="project-connector-layer"
                viewBox="0 0 100 100"
                preserveAspectRatio="none"
                aria-hidden="true"
                style={{ transform: `scale(${zoom})` }}
              >
                {postgresResource &&
                  appServices.map((service) => {
                    if (!service.databaseConnection) {
                      return null
                    }
                    const serviceNode = nodes.find(
                      (node) => node.id === appServiceNodeId(service.id)
                    )
                    const databaseNode = nodes.find(
                      (node) => node.id === "postgres"
                    )
                    if (!serviceNode || !databaseNode) {
                      return null
                    }
                    const connector = appServiceDatabaseConnectorId(service.id)
                    const from = databaseNode.position
                    const to = serviceNode.position
                    const midTop = Math.max(from.top, to.top - 6)
                    return (
                      <path
                        key={connector}
                        className={connectorClassName(connector, selectedNode)}
                        data-connector={connector}
                        d={`M${from.left} ${from.top} V${midTop} H${to.left} V${to.top}`}
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
              {!resourcesLoading &&
                !resourceError &&
                resourceNodes.length === 0 && (
                  <div className="project-canvas-empty">
                    <strong>Nothing deployed yet</strong>
                    <span>
                      Resources you add appear here with their private network
                      links.
                    </span>
                    <Button
                      type="button"
                      size="sm"
                      variant="outline"
                      onClick={() => setPostgresDialogOpen(true)}
                    >
                      <DatabaseIcon data-icon="inline-start" />
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
                      isDataNode(node) ? "is-data" : "is-service",
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
                      <span className="project-node-logo" aria-hidden="true">
                        <ResourceIcon node={node} />
                      </span>
                      <div className="project-node-text">
                        <h2 className="project-node-heading">{node.title}</h2>
                        <p className="project-node-type">{node.type}</p>
                      </div>
                    </div>
                    {node.subtitle && node.subtitle !== node.title && (
                      <p className="project-node-subtitle">{node.subtitle}</p>
                    )}
                    <div className="project-node-footer">
                      <StatusBadge status={node.status} />
                      <span className="project-node-volume">
                        <HardDriveIcon aria-hidden="true" />
                        <span>{node.volume}</span>
                      </span>
                    </div>
                  </article>
                ))}
              </div>

              <div className="project-selection-hint" aria-hidden="true">
                Drag to arrange · click to inspect
              </div>

              <div className="project-zoom-dock" aria-label="Canvas controls">
                <button
                  className="project-zoom-button"
                  type="button"
                  aria-label="Zoom out"
                  onClick={() => updateZoom(zoom - 0.1, "Zoomed out")}
                >
                  <MinusIcon aria-hidden="true" />
                </button>
                <span className="project-zoom-readout" aria-live="polite">
                  {Math.round(zoom * 100)}%
                </span>
                <button
                  className="project-zoom-button"
                  type="button"
                  aria-label="Zoom in"
                  onClick={() => updateZoom(zoom + 0.1, "Zoomed in")}
                >
                  <PlusIcon aria-hidden="true" />
                </button>
                <span className="project-zoom-divider" aria-hidden="true" />
                <button
                  className="project-zoom-button"
                  type="button"
                  aria-label="Fit topology to view"
                  onClick={() => updateZoom(1, "View fitted")}
                >
                  <Maximize2Icon aria-hidden="true" />
                </button>
              </div>
            </section>
          </>
        )}
      </main>

      {selectedNodeData?.resource?.resourceType === "images" ? (
        <ImageStoreWorkspace
          store={selectedNodeData.resource}
          workspaceId={workspaceId}
          projectSlug={projectSlug}
          onClose={closeResourceWorkspace}
          onChanged={() => {
            void refreshImageStores()
          }}
        />
      ) : selectedNodeData && isInfrastructureNode(selectedNodeData) ? (
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
      ) : null}

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

      <ImageStoreCreateDialog
        key={imageDialogOpen ? "image-dialog-open" : "image-dialog-closed"}
        workspaceId={workspaceId}
        projectSlug={projectSlug}
        open={imageDialogOpen}
        onOpenChange={setImageDialogOpen}
        onCreated={handleImageStoreCreated}
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
          <CheckIcon aria-hidden="true" />
          {toast}
        </div>
      )}
    </AppShell>
  )
}

type AddResourceActions = Record<
  "postgres" | "redis" | "images" | "app",
  () => void
>

function StatusBadge({ status }: { status: string }) {
  return (
    <span className={cn("status-badge", `status-${statusSlug(status)}`)}>
      <span className="status-badge-dot" aria-hidden="true" />
      {status}
    </span>
  )
}

function statusSlug(status: string) {
  return status.toLowerCase().replace(/[^a-z0-9]+/g, "-")
}

function isDataNode(node: TopologyNode) {
  return (
    node.resource?.resourceType === "postgres" ||
    node.resource?.resourceType === "redis" ||
    node.resource?.resourceType === "images"
  )
}

function ViewState({
  loading,
  error,
}: {
  loading: boolean
  error: string | null
}) {
  return (
    <>
      {error && (
        <div className="project-view-error" role="alert">
          <strong>Resources unavailable</strong>
          <span>{error}</span>
        </div>
      )}
      {loading && (
        <div className="loading-row" role="status">
          <Spinner />
          <span>Loading resources</span>
        </div>
      )}
    </>
  )
}

function ResourcesWorkspace({
  project,
  nodes,
  loading,
  error,
  onOpenResource,
  addActions,
}: {
  project: Project
  nodes: TopologyNode[]
  loading: boolean
  error: string | null
  onOpenResource: (node: TopologyNode) => void
  addActions: AddResourceActions
}) {
  const [search, setSearch] = useState("")
  const [filter, setFilter] = useState<"all" | "services" | "data">("all")
  const normalizedSearch = search.trim().toLowerCase()
  const filteredNodes = nodes.filter((node) => {
    const isDataStore = isDataNode(node)
    const matchesFilter =
      filter === "all" || (filter === "data" ? isDataStore : !isDataStore)
    const matchesSearch =
      !normalizedSearch ||
      `${node.title} ${node.type} ${node.subtitle ?? ""}`
        .toLowerCase()
        .includes(normalizedSearch)
    return matchesFilter && matchesSearch
  })
  const counts = {
    total: nodes.length,
    online: nodes.filter((node) => ["Online", "Ready"].includes(node.status))
      .length,
    creating: nodes.filter((node) => node.status === "Creating").length,
    attention: nodes.filter((node) => node.status === "Needs attention").length,
  }

  return (
    <div className="app-content">
      <section
        className="app-page project-content-view"
        aria-labelledby="resources-title"
      >
        <div className="page-header">
          <div>
            <h1 id="resources-title">Resources</h1>
            <p>Manage app services and data stores for this project.</p>
          </div>
        </div>

        {!loading && nodes.length > 0 && (
          <dl className="project-stats" aria-label="Resource health">
            <div>
              <dt>Resources</dt>
              <dd>{counts.total}</dd>
            </div>
            <div className="is-online">
              <dt>Healthy</dt>
              <dd>{counts.online}</dd>
            </div>
            <div className="is-creating">
              <dt>Deploying</dt>
              <dd>{counts.creating}</dd>
            </div>
            <div className={cn(counts.attention > 0 && "is-attention")}>
              <dt>Needs attention</dt>
              <dd>{counts.attention}</dd>
            </div>
          </dl>
        )}

        {nodes.length > 0 && (
          <div className="project-resource-toolbar">
            <div
              className="segmented"
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
                  onClick={() => setFilter(value)}
                >
                  {label}
                </button>
              ))}
            </div>
            <label className="search-field">
              <SearchIcon aria-hidden="true" />
              <span className="sr-only">Search resources</span>
              <input
                type="search"
                value={search}
                placeholder="Search resources"
                onChange={(event) => setSearch(event.target.value)}
              />
            </label>
          </div>
        )}

        <ViewState loading={loading} error={error} />

        {loading ? null : nodes.length === 0 && !error ? (
          <div className="empty-panel">
            <span className="empty-icon" aria-hidden="true">
              <LayoutGridIcon />
            </span>
            <div>
              <h2>No resources yet</h2>
              <p>
                Add an app service, HTML page, PostgreSQL database, Redis store,
                or image store to get started.
              </p>
            </div>
            <div className="project-quick-add">
              <QuickAddTile
                icon={<BoxIcon />}
                title="App service"
                description="Container image or HTML page"
                onClick={addActions.app}
              />
              <QuickAddTile
                icon={<DatabaseIcon />}
                title="PostgreSQL"
                description="Dedicated database"
                onClick={addActions.postgres}
              />
              <QuickAddTile
                icon={<LayersIcon />}
                title="Redis store"
                description="Cache and queues"
                onClick={addActions.redis}
              />
              <QuickAddTile
                icon={<ImageIcon />}
                title="Image store"
                description="Resized image delivery"
                onClick={addActions.images}
              />
            </div>
          </div>
        ) : filteredNodes.length === 0 && !error ? (
          <div className="empty-panel project-empty-compact">
            <div>
              <h2>No matching resources</h2>
              <p>Try another search or filter for {project.name}.</p>
            </div>
          </div>
        ) : nodes.length > 0 ? (
          <div className="data-table-wrap">
            <table className="data-table project-resource-table">
              <caption className="sr-only">
                Resources for {project.name}
              </caption>
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col" className="project-resource-type-cell">
                    Type
                  </th>
                  <th scope="col">Status</th>
                  <th scope="col">
                    <span className="sr-only">Open</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {filteredNodes.map((node) => (
                  <tr key={node.id} onClick={() => onOpenResource(node)}>
                    <th scope="row">
                      <span className="project-resource-name-cell">
                        <span
                          className={cn(
                            "project-resource-icon",
                            isDataNode(node) && "is-data"
                          )}
                          aria-hidden="true"
                        >
                          <ResourceIcon node={node} />
                        </span>
                        <span className="project-resource-name">
                          <strong>{node.title}</strong>
                          {node.subtitle && node.subtitle !== node.title && (
                            <span className="project-resource-subtitle">
                              {node.subtitle}
                            </span>
                          )}
                        </span>
                      </span>
                    </th>
                    <td className="project-resource-type-cell">{node.type}</td>
                    <td>
                      <StatusBadge status={node.status} />
                    </td>
                    <td className="project-resource-action-cell">
                      <button
                        type="button"
                        className="project-resource-open"
                        aria-label={`Open ${node.title}`}
                        onClick={(event) => {
                          event.stopPropagation()
                          onOpenResource(node)
                        }}
                      >
                        <span>Open</span>
                        <ChevronRightIcon aria-hidden="true" />
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
      </section>
    </div>
  )
}

function QuickAddTile({
  icon,
  title,
  description,
  onClick,
}: {
  icon: ReactNode
  title: string
  description: string
  onClick: () => void
}) {
  return (
    <button
      type="button"
      className="project-quick-add-tile"
      aria-label={`New ${title}`}
      onClick={onClick}
    >
      <span aria-hidden="true">{icon}</span>
      <strong>{title}</strong>
      <small>{description}</small>
    </button>
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
    <div className="app-content">
      <section
        className="app-page project-content-view"
        aria-labelledby="metrics-title"
      >
        <div className="page-header">
          <div>
            <h1 id="metrics-title">Metrics</h1>
            <p>
              Choose a PostgreSQL database or app service to view its live
              metrics.
            </p>
          </div>
        </div>

        <ViewState loading={loading} error={error} />

        {loading ? null : metricNodes.length === 0 && !error ? (
          <div className="empty-panel">
            <span className="empty-icon" aria-hidden="true">
              <ActivityIcon />
            </span>
            <div>
              <h2>No metrics resources yet</h2>
              <p>
                Add a PostgreSQL database or app service to view runtime metrics
                for {project.name}.
              </p>
            </div>
          </div>
        ) : (
          <ul className="project-metric-grid">
            {metricNodes.map((node) => (
              <li key={node.id}>
                <button
                  type="button"
                  className="project-metric-card"
                  aria-label={`View metrics for ${node.title}`}
                  onClick={() => onOpenMetrics(node)}
                >
                  <span className="project-metric-card-head">
                    <span
                      className={cn(
                        "project-resource-icon",
                        isDataNode(node) && "is-data"
                      )}
                      aria-hidden="true"
                    >
                      <ResourceIcon node={node} />
                    </span>
                    <StatusBadge status={node.status} />
                  </span>
                  <span className="project-metric-card-body">
                    <strong>{node.title}</strong>
                    <span>{node.type}</span>
                  </span>
                  <span className="project-metric-card-foot">
                    <span>
                      {node.resource?.resourceType === "app"
                        ? "CPU · memory · requests · latency"
                        : "CPU · memory · volume · I/O"}
                    </span>
                    <span className="project-metric-card-cta">
                      View metrics
                      <ArrowUpRightIcon aria-hidden="true" />
                    </span>
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  )
}

function ResourceIcon({ node }: { node: TopologyNode }) {
  if (node.type === "HTML page") {
    return <FileCodeIcon aria-hidden="true" />
  }
  if (node.resource?.resourceType === "images") {
    return <ImageIcon aria-hidden="true" />
  }
  if (node.resource?.resourceType === "postgres") {
    return <DatabaseIcon aria-hidden="true" />
  }
  if (node.resource?.resourceType === "redis") {
    return <LayersIcon aria-hidden="true" />
  }
  return <BoxIcon aria-hidden="true" />
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
        appServiceDeploymentEventsUrl(workspaceId, projectSlug, deployment.id),
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
    <div className="app-content">
      <section
        className="app-page project-logs-shell"
        aria-label={`${project.name} logs`}
      >
        <div className="page-header">
          <div>
            <h1>Logs</h1>
            <p>
              Deployment events for {project.name} ·{" "}
              <span className="mono">{environment}</span>
            </p>
          </div>
        </div>
        <div className="project-logs-panel">
          <div className="project-logs-toolbar">
            <label className="project-select">
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
            <label className="search-field">
              <SearchIcon aria-hidden="true" />
              <span className="sr-only">Search logs</span>
              <input
                type="search"
                value={search}
                placeholder="Filter messages…"
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
          <div className="project-logs-stream" role="log" aria-live="polite">
            {visibleDeployments.length > 0 ? (
              visibleDeployments.map(({ service, deployment }) => (
                <div key={service.id} className="project-logs-deployment">
                  <strong>{service.name}</strong>
                  <AppServiceDeploymentLogs deployment={deployment} compact />
                  {!live && (
                    <span className="project-logs-filter-note">
                      Live tail paused; showing the last saved deployment
                      snapshot.
                    </span>
                  )}
                </div>
              ))
            ) : (
              <div className="project-logs-empty">
                <span className="empty-icon" aria-hidden="true">
                  <ScrollTextIcon />
                </span>
                <h2>No logs yet</h2>
                <p>
                  Logs will appear here once{" "}
                  {resourceFilter === "all" ? "a resource" : "this resource"}{" "}
                  starts handling traffic.
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
        </div>
      </section>
    </div>
  )
}

function ProjectAddOption({
  label,
  description,
  icon,
  disabled = false,
  onClick,
}: {
  label: string
  description: string
  icon: ReactNode
  disabled?: boolean
  onClick: () => void
}) {
  return (
    <button
      className="project-menu-option"
      type="button"
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
    >
      <span className="project-menu-icon" aria-hidden="true">
        {icon}
      </span>
      <span className="project-menu-text" aria-hidden="true">
        <strong>{label}</strong>
        <small>{description}</small>
      </span>
    </button>
  )
}

function appServiceNodeId(serviceId: string) {
  return `app:${serviceId}`
}

function imageStoreNodeId(storeId: string) {
  return `images:${storeId}`
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
