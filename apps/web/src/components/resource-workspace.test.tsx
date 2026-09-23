import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { ResourceWorkspace } from "@/components/resource-workspace"

const mocks = vi.hoisted(() => ({
  createDatabaseTable: vi.fn(),
  executeDatabaseQuery: vi.fn(),
  getAppServiceMetrics: vi.fn(),
  getDatabaseConfig: vi.fn(),
  getDatabaseMetrics: vi.fn(),
  getDatabaseStats: vi.fn(),
  getDatabaseTableData: vi.fn(),
  getAppServiceLogs: vi.fn(),
  getHtmlPageAnalytics: vi.fn(),
  getHtmlPageIndex: vi.fn(),
  updateHtmlPage: vi.fn(),
  deleteKnotreeRegistryConnection: vi.fn(),
  listAppServices: vi.fn(),
  listDatabaseTables: vi.fn(),
  listPostgresResources: vi.fn(),
  retryPostgresResource: vi.fn(),
  updateAppService: vi.fn(),
  updateAppServiceAutoDeploy: vi.fn(),
  updateAppServiceDatabase: vi.fn(),
  updateAppServicePublicAccess: vi.fn(),
  updateKnotreeRegistryConnection: vi.fn(),
}))

vi.mock("@/lib/resources", () => mocks)

describe("ResourceWorkspace database pane", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listDatabaseTables.mockResolvedValue([
      {
        schemaName: "public",
        tableName: "1",
        estimatedRows: 0,
        sizeBytes: 8192,
      },
    ])
    mocks.getDatabaseTableData.mockResolvedValue({
      schemaName: "public",
      tableName: "1",
      columns: [{ name: "id", dataType: "bigint", nullable: false }],
      rows: [],
      limit: 50,
      offset: 0,
      rowCount: 0,
    })
    mocks.getDatabaseMetrics.mockResolvedValue({
      provider: "docker",
      systemMetricsAvailable: true,
      systemMetricsMessage: null,
      sampleIntervalSeconds: 5,
      retentionSeconds: 2_592_000,
      range: "24h",
      fromTimestamp: 1_697_408_000,
      toTimestamp: 1_700_000_000,
      resolutionSeconds: 288,
      points: [
        {
          timestamp: 1_700_000_000,
          cpuPercent: 3.33,
          memoryUsedBytes: 30_910_000,
          memoryLimitBytes: 4_169_000_000,
          volumeUsedBytes: 32_000_000,
          volumeCapacityBytes: 400_000_000_000,
          networkReceiveBytes: 229_000,
          networkTransmitBytes: 120_000,
          diskReadBytes: 457_000_000,
          diskWriteBytes: 56_600_000,
        },
      ],
    })
    mocks.getAppServiceMetrics.mockResolvedValue({
      provider: "docker",
      systemMetricsAvailable: true,
      systemMetricsMessage: null,
      sampleIntervalSeconds: 5,
      retentionSeconds: 2_592_000,
      range: "24h",
      fromTimestamp: 1_697_408_000,
      toTimestamp: 1_700_000_000,
      resolutionSeconds: 288,
      points: [
        {
          timestamp: 1_700_000_000,
          cpuPercent: 3.33,
          memoryUsedBytes: 30_910_000,
          memoryLimitBytes: 1_000_000_000,
          volumeUsedBytes: 32_000_000,
          volumeCapacityBytes: 10_737_418_240,
          networkReceiveBytes: 229_000,
          networkTransmitBytes: 120_000,
          diskReadBytes: 457_000_000,
          diskWriteBytes: 56_600_000,
          publicNetworkReceiveBytes: 45_000,
          publicNetworkTransmitBytes: 125_000,
          requests: 1_248,
          responseTimeMs: 86,
          requestErrorRate: 1.2,
        },
      ],
    })
    mocks.getAppServiceLogs.mockResolvedValue({
      appServiceId: "app-resource-id",
      containerName: "knotree-app-project",
      status: "ready",
      running: true,
      lines: [
        "2026-09-13T12:00:00Z listening on 0.0.0.0:3000",
        "2026-09-13T12:00:01Z GET / 200",
      ],
      message: null,
    })
    mocks.listPostgresResources.mockResolvedValue([])
  })

  it("lets a failed PostgreSQL resource be deployed again", async () => {
    const user = userEvent.setup()
    const onPostgresUpdated = vi.fn()
    const onToast = vi.fn()
    const resource = {
      id: "resource-id",
      name: "Analytics",
      resourceType: "postgres" as const,
      status: "error" as const,
      databaseName: "knotree_db_analytics",
      username: "knotree_role_analytics",
      host: "knotree-cloud-knotree-api-pg",
      port: 5432,
      connectionString: null,
      clusterProvider: "kubernetes" as const,
      errorMessage:
        "The Kubernetes cluster has no schedulable capacity for this database.",
    }
    const retriedResource = { ...resource, status: "provisioning" as const }
    mocks.retryPostgresResource.mockResolvedValue(retriedResource)

    render(
      <ResourceWorkspace
        node={{
          id: "postgres",
          title: "Analytics",
          type: "PostgreSQL database",
          volume: "analytics-volume",
          status: "ERROR",
          resource,
        }}
        environment="production"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
        onPostgresUpdated={onPostgresUpdated}
      />
    )

    const dialog = screen.getByRole("dialog")
    expect(
      within(dialog).getByText(/no schedulable capacity/i)
    ).toBeInTheDocument()
    await user.click(
      within(dialog).getByRole("button", { name: "Deploy PostgreSQL again" })
    )

    await waitFor(() => {
      expect(mocks.retryPostgresResource).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "resource-id"
      )
    })
    expect(onPostgresUpdated).toHaveBeenCalledWith(retriedResource)
    expect(onToast).toHaveBeenCalledWith("Postgres redeploy started")
  })

  it("uses the first real table in the starter query", async () => {
    const user = userEvent.setup()
    render(
      <ResourceWorkspace
        node={{
          id: "postgres",
          title: "Analytics",
          type: "PostgreSQL database",
          volume: "analytics-volume",
          status: "ACTIVE",
          resource: {
            id: "resource-id",
            name: "Analytics",
            resourceType: "postgres",
            status: "ready",
            databaseName: "knotree_db_analytics",
            username: "knotree_role_analytics",
            host: "127.0.0.1",
            port: 5432,
            connectionString: "postgres://localhost/analytics",
            clusterProvider: "docker",
            clusterName: "knotree-pg-analytics",
          },
        }}
        environment="production"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={vi.fn()}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Database" }))

    expect(
      await within(dialog).findByRole("button", { name: /^1/ })
    ).toBeInTheDocument()
    expect(within(dialog).getByLabelText("SQL query")).toHaveValue(
      'SELECT * FROM "public"."1" LIMIT 50'
    )

    await user.click(within(dialog).getByRole("button", { name: /^1/ }))
    expect(await within(dialog).findByText("public.1")).toBeInTheDocument()
    expect(mocks.getDatabaseTableData).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "test-2",
      "resource-id",
      "1",
      "public",
      50,
      0
    )
  })

  it("renders live database runtime metrics", async () => {
    const user = userEvent.setup()
    render(
      <ResourceWorkspace
        node={{
          id: "postgres",
          title: "Analytics",
          type: "PostgreSQL database",
          volume: "analytics-volume",
          status: "ACTIVE",
          resource: {
            id: "resource-id",
            name: "Analytics",
            resourceType: "postgres",
            status: "ready",
            databaseName: "knotree_db_analytics",
            username: "knotree_role_analytics",
            host: "127.0.0.1",
            port: 5432,
            connectionString: "postgres://localhost/analytics",
            clusterProvider: "docker",
            clusterName: "knotree-pg-analytics",
          },
        }}
        environment="production"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={vi.fn()}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Metrics" }))

    expect(await within(dialog).findByText("3.33%")).toBeInTheDocument()
    expect(within(dialog).getByText("Network I/O")).toBeInTheDocument()
    expect(within(dialog).getByText("Disk I/O")).toBeInTheDocument()
    const cpuChart = within(dialog).getByRole("img", {
      name: "CPU usage, last 24 hours",
    })
    fireEvent.mouseMove(cpuChart, { clientX: 1 })
    expect(within(dialog).getByRole("status")).toHaveTextContent("3.33%")
    expect(mocks.getDatabaseMetrics).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "test-2",
      "resource-id",
      "24h"
    )

    await user.selectOptions(
      within(dialog).getByLabelText("Metric time range"),
      "7d"
    )
    await waitFor(() => {
      expect(mocks.getDatabaseMetrics).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "resource-id",
        "7d"
      )
    })

    await user.selectOptions(
      within(dialog).getByLabelText("Metric time range"),
      "30d"
    )
    await waitFor(() => {
      expect(mocks.getDatabaseMetrics).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "resource-id",
        "30d"
      )
    })
  })

  it("renders live app service runtime metrics", async () => {
    const user = userEvent.setup()
    const onToast = vi.fn()
    render(
      <ResourceWorkspace
        node={{
          id: "app:app-resource-id",
          title: "Web app",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "Web app",
            resourceType: "app",
            status: "ready",
            image: "nginx:alpine",
            imageSource: "public",
            appPort: 80,
            host: "localhost",
            port: 49152,
            serviceUrl: "http://localhost:49152",
            containerName: "knotree-app-app-resource-id",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Metrics" }))

    expect(await within(dialog).findByText("3.33%")).toBeInTheDocument()
    expect(within(dialog).getByText("Network I/O")).toBeInTheDocument()
    expect(
      within(dialog).getByText("Public Network Traffic")
    ).toBeInTheDocument()
    expect(
      within(dialog).getByRole("heading", { name: "Requests" })
    ).toBeInTheDocument()
    expect(
      within(dialog).getByRole("heading", { name: "Response Time" })
    ).toBeInTheDocument()
    expect(
      within(dialog).getByRole("heading", { name: "Request Error Rate" })
    ).toBeInTheDocument()
    expect(within(dialog).getByText("1.20%")).toBeInTheDocument()
    expect(within(dialog).getByText("Telemetry active")).toBeInTheDocument()
    expect(within(dialog).getByText("1 samples · every 5s")).toBeInTheDocument()
    expect(within(dialog).getByText("Resolution")).toBeInTheDocument()
    expect(within(dialog).getByText("30 days")).toBeInTheDocument()
    const requestsCard = within(dialog)
      .getByRole("heading", { name: "Requests" })
      .closest("article")!
    expect(within(requestsCard).getAllByText("1,248")).toHaveLength(2)
    expect(
      within(requestsCard).getByRole("img").querySelector("text")
    ).toHaveTextContent("1,248")
    const responseTimeCard = within(dialog)
      .getByRole("heading", { name: "Response Time" })
      .closest("article")!
    expect(within(responseTimeCard).getAllByText("86 ms")).toHaveLength(2)
    expect(
      within(responseTimeCard).getByRole("img").querySelector("text")
    ).toHaveTextContent("86 ms")
    expect(within(dialog).getByText("Disk I/O")).toBeInTheDocument()
    expect(mocks.getAppServiceMetrics).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "test-2",
      "app-resource-id",
      "24h"
    )

    await user.click(within(dialog).getByRole("button", { name: "Live" }))
    expect(
      within(dialog).getByRole("button", { name: "Paused" })
    ).toBeInTheDocument()
    expect(onToast).toHaveBeenCalledWith("Live metrics paused")
  })

  it("shows the manually assigned private database connection", async () => {
    const user = userEvent.setup()
    render(
      <ResourceWorkspace
        node={{
          id: "project",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "App service",
            resourceType: "app",
            status: "ready",
            image: "node:22-alpine",
            imageSource: "public",
            appPort: 3000,
            host: "localhost",
            port: 49152,
            serviceUrl: "http://localhost:49152",
            containerName: "knotree-app-project",
            databaseConnection: {
              resourceId: "resource-id",
              name: "Postgres",
              databaseName: "knotree_db_project",
              username: "knotree_role_project",
              networkName: "knotree-net-project",
              host: "postgres",
              port: 5432,
              environmentVariables: [
                "DATABASE_URL",
                "PGHOST",
                "PGPORT",
                "PGDATABASE",
                "PGUSER",
                "PGPASSWORD",
              ],
            },
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={vi.fn()}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    expect(
      within(dialog).getByText("Postgres connection assigned")
    ).toBeInTheDocument()
    expect(within(dialog).getByText("postgres:5432")).toBeInTheDocument()

    await user.click(within(dialog).getByRole("tab", { name: "Variables" }))
    expect(await within(dialog).findByText("PGDATABASE")).toBeInTheDocument()
    expect(within(dialog).getByText("knotree_db_project")).toBeInTheDocument()
  })

  it("loads runtime logs for the selected app service", async () => {
    const user = userEvent.setup()
    render(
      <ResourceWorkspace
        node={{
          id: "project",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "App service",
            resourceType: "app",
            status: "ready",
            image: "node:22-alpine",
            imageSource: "public",
            appPort: 3000,
            host: "localhost",
            port: 49152,
            serviceUrl: "http://localhost:49152",
            containerName: "knotree-app-project",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={vi.fn()}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Console" }))

    expect(await within(dialog).findByRole("log")).toHaveTextContent(
      "2026-09-13T12:00:01Z GET / 200"
    )
    expect(mocks.getAppServiceLogs).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "test-2",
      "app-resource-id"
    )
    expect(
      within(dialog).getByRole("button", { name: "Refresh app service logs" })
    ).toBeInTheDocument()
  })
})

describe("ResourceWorkspace settings pane", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listPostgresResources.mockResolvedValue([])
  })

  it("redeploys the app when the container port is saved", async () => {
    const user = userEvent.setup()
    const onAppServiceUpdated = vi.fn()
    const onToast = vi.fn()
    mocks.updateAppService.mockResolvedValue({
      id: "app-resource-id",
      name: "App service",
      resourceType: "app",
      status: "ready",
      image: "nginxdemos/hello",
      imageSource: "public",
      appPort: 80,
      host: "localhost",
      port: 59601,
      serviceUrl: "http://localhost:59601",
      containerName: "knotree-app-project",
    })

    render(
      <ResourceWorkspace
        node={{
          id: "project",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "App service",
            resourceType: "app",
            status: "ready",
            image: "nginxdemos/hello",
            imageSource: "public",
            appPort: 8080,
            host: "localhost",
            port: 51952,
            serviceUrl: "http://localhost:51952",
            publicDomain: "app-0123456789abcdef.knotree.org",
            containerName: "knotree-app-project",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
        onAppServiceUpdated={onAppServiceUpdated}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    expect(
      within(dialog).getByText("app-0123456789abcdef.knotree.org")
    ).toBeInTheDocument()
    const portInput = within(dialog).getByLabelText("Container port")
    expect(portInput).toHaveValue(8080)
    await user.clear(portInput)
    await user.type(portInput, "80")
    await user.click(
      within(dialog).getByRole("button", { name: "Save and redeploy" })
    )

    await waitFor(() => {
      expect(mocks.updateAppService).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "app-resource-id",
        { appPort: 80 }
      )
    })
    expect(onAppServiceUpdated).toHaveBeenCalledWith(
      expect.objectContaining({ appPort: 80, port: 59601 })
    )
    expect(onToast).toHaveBeenCalledWith(
      "Container port updated. The app was redeployed."
    )
  })

  it("toggles automatic GitHub image deploys from settings", async () => {
    const user = userEvent.setup()
    const onAppServiceUpdated = vi.fn()
    const onToast = vi.fn()
    mocks.updateAppServiceAutoDeploy.mockResolvedValue({
      id: "app-resource-id",
      name: "App service",
      resourceType: "app",
      status: "ready",
      image: "ghcr.io/acme/app:latest",
      imageSource: "github",
      appPort: 3000,
      host: "localhost",
      port: 59601,
      serviceUrl: "http://localhost:59601",
      containerName: "knotree-app-project",
      autoDeployEnabled: false,
      deployedImageDigest: "sha256:old",
    })

    render(
      <ResourceWorkspace
        node={{
          id: "project",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "App service",
            resourceType: "app",
            status: "ready",
            image: "ghcr.io/acme/app:latest",
            imageSource: "github",
            appPort: 3000,
            host: "localhost",
            port: 51952,
            serviceUrl: "http://localhost:51952",
            containerName: "knotree-app-project",
            autoDeployEnabled: true,
            deployedImageDigest: "sha256:old",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
        onAppServiceUpdated={onAppServiceUpdated}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    const toggle = within(dialog).getByRole("checkbox", {
      name: "Auto deploy new GitHub images",
    })
    expect(toggle).toBeChecked()
    await user.click(toggle)

    await waitFor(() => {
      expect(mocks.updateAppServiceAutoDeploy).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "app-resource-id",
        { enabled: false }
      )
    })
    expect(onAppServiceUpdated).toHaveBeenCalledWith(
      expect.objectContaining({ autoDeployEnabled: false })
    )
    expect(onToast).toHaveBeenCalledWith(
      "Automatic GitHub image deploys disabled."
    )
  })

  it("controls Knotree Registry auto-deploy, rotates its token, and disconnects safely", async () => {
    const user = userEvent.setup()
    const onAppServiceUpdated = vi.fn()
    const onToast = vi.fn()
    const disconnectedService = {
      id: "app-resource-id",
      name: "App service",
      resourceType: "app" as const,
      status: "ready" as const,
      image: "registry.knotree.com/team/api:production",
      imageSource: "knotree_registry" as const,
      appPort: 3000,
      host: "localhost",
      port: 59601,
      serviceUrl: "http://localhost:59601",
      containerName: "knotree-app-app-resource-id",
      autoDeployEnabled: false,
      deployedImageDigest: "sha256:0123456789abcdef",
      autoDeployCheckedAt: "2026-09-23T09:00:00Z",
      registryConnectionId: null,
    }
    mocks.updateAppServiceAutoDeploy.mockResolvedValue({
      ...disconnectedService,
      autoDeployEnabled: true,
      registryConnectionId: "registry-connection-1",
    })
    mocks.updateKnotreeRegistryConnection.mockResolvedValue({
      id: "registry-connection-1",
      registryHost: "registry.knotree.com",
      username: "service-user",
      repository: "team/api",
      verifiedAt: "2026-09-23T09:10:00Z",
    })
    mocks.deleteKnotreeRegistryConnection.mockResolvedValue(undefined)
    mocks.listAppServices.mockResolvedValue([disconnectedService])

    render(
      <ResourceWorkspace
        node={{
          id: "app:app-resource-id",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            ...disconnectedService,
            registryConnectionId: "registry-connection-1",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
        onAppServiceUpdated={onAppServiceUpdated}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    expect(within(dialog).getByText("sha256:0123456789abcdef")).toBeInTheDocument()
    const toggle = within(dialog).getByRole("checkbox", {
      name: "Auto deploy new Knotree Registry images",
    })
    await user.click(toggle)
    await waitFor(() => {
      expect(mocks.updateAppServiceAutoDeploy).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "app-resource-id",
        { enabled: true }
      )
    })

    await user.type(
      within(dialog).getByLabelText("Replace pull token"),
      "replacement-pull-pat"
    )
    await user.click(within(dialog).getByRole("button", { name: "Update token" }))
    await waitFor(() => {
      expect(mocks.updateKnotreeRegistryConnection).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "registry-connection-1",
        "replacement-pull-pat"
      )
    })
    expect(within(dialog).getByLabelText("Replace pull token")).toHaveValue("")

    await user.click(
      within(dialog).getByRole("button", { name: "Disconnect Knotree Registry" })
    )
    await waitFor(() => {
      expect(mocks.deleteKnotreeRegistryConnection).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "registry-connection-1"
      )
      expect(mocks.listAppServices).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2"
      )
    })
    expect(onAppServiceUpdated).toHaveBeenCalledWith(disconnectedService)
    expect(onToast).toHaveBeenCalledWith(
      "Cloud connection removed. Revoke the PAT in Knotree Registry too if you no longer need it. The running service stays up; future Cloud pulls and auto-deploys are stopped."
    )
  })

  it("keeps the current port when the value is invalid", async () => {
    const user = userEvent.setup()
    render(
      <ResourceWorkspace
        node={{
          id: "project",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "App service",
            resourceType: "app",
            status: "ready",
            image: "nginxdemos/hello",
            imageSource: "public",
            appPort: 80,
            host: "localhost",
            port: 59601,
            serviceUrl: "http://localhost:59601",
            containerName: "knotree-app-project",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={vi.fn()}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    const portInput = within(dialog).getByLabelText("Container port")
    fireEvent.change(portInput, { target: { value: "0" } })
    fireEvent.submit(portInput.closest("form")!)

    expect(mocks.updateAppService).not.toHaveBeenCalled()
    expect(
      within(dialog).getByText("Use a container port between 1 and 65535.")
    ).toBeInTheDocument()
  })

  it("assigns a PostgreSQL resource from the service settings menu", async () => {
    const user = userEvent.setup()
    const onAppServiceUpdated = vi.fn()
    const onToast = vi.fn()
    mocks.listPostgresResources.mockResolvedValue([
      {
        id: "resource-id",
        name: "Analytics",
        resourceType: "postgres",
        status: "ready",
        databaseName: "knotree_db_project",
        username: "knotree_role_project",
        host: "127.0.0.1",
        port: 5432,
        connectionString: null,
        clusterProvider: "docker",
      },
    ])
    mocks.updateAppServiceDatabase.mockResolvedValue({
      id: "app-resource-id",
      name: "App service",
      resourceType: "app",
      status: "ready",
      image: "nginx:alpine",
      imageSource: "public",
      appPort: 3000,
      host: "localhost",
      port: 59601,
      serviceUrl: "http://localhost:59601",
      containerName: "knotree-app-app-resource-id",
      databaseConnection: {
        resourceId: "resource-id",
        name: "Analytics",
        databaseName: "knotree_db_project",
        username: "knotree_role_project",
        networkName: "knotree-net-project",
        host: "postgres",
        port: 5432,
        environmentVariables: ["DATABASE_URL"],
      },
    })

    render(
      <ResourceWorkspace
        node={{
          id: "app:app-resource-id",
          title: "App service",
          type: "Docker app service",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "App service",
            resourceType: "app",
            status: "ready",
            image: "nginx:alpine",
            imageSource: "public",
            appPort: 3000,
            host: "localhost",
            port: 59601,
            serviceUrl: "http://localhost:59601",
            containerName: "knotree-app-app-resource-id",
          },
        }}
        environment="development"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
        onAppServiceUpdated={onAppServiceUpdated}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    const databaseSelect = await within(dialog).findByLabelText(
      "PostgreSQL resource for app service"
    )
    await user.selectOptions(databaseSelect, "resource-id")

    await waitFor(() => {
      expect(mocks.updateAppServiceDatabase).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "app-resource-id",
        { databaseResourceId: "resource-id" }
      )
    })
    expect(onAppServiceUpdated).toHaveBeenCalledWith(
      expect.objectContaining({ databaseConnection: expect.any(Object) })
    )
    expect(onToast).toHaveBeenCalledWith(
      "Postgres connection assigned. The service was redeployed."
    )
  })

  it("wires public-domain enable and a custom rate limit", async () => {
    const user = userEvent.setup()
    mocks.updateAppServicePublicAccess.mockResolvedValue({
      id: "app-resource-id",
      name: "Web",
      resourceType: "app",
      status: "ready",
      image: "nginx:alpine",
      imageSource: "public",
      appPort: 80,
      host: "localhost",
      port: 32768,
      serviceUrl: "https://app-0123456789abcdef.knotree.org",
      publicDomain: "app-0123456789abcdef.knotree.org",
      publicAccessEnabled: true,
      rateLimitRpm: 120,
      containerName: "knotree-app-app-resource-id",
    })
    const onToast = vi.fn()
    render(
      <ResourceWorkspace
        node={{
          id: "app",
          title: "Web",
          type: "App service",
          volume: "web-volume",
          status: "ACTIVE",
          resource: {
            id: "app-resource-id",
            name: "Web",
            resourceType: "app",
            status: "ready",
            image: "nginx:alpine",
            imageSource: "public",
            appPort: 80,
            host: "localhost",
            port: 32768,
            serviceUrl: null,
            publicDomain: null,
            publicAccessEnabled: false,
            rateLimitRpm: 60,
            containerName: "knotree-app-app-resource-id",
          },
        }}
        environment="production"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    await user.click(within(dialog).getByLabelText("Enable public hostname"))
    const rateLimit = within(dialog).getByLabelText("App service rate limit")
    await user.clear(rateLimit)
    await user.type(rateLimit, "120")
    await user.click(
      within(dialog).getByRole("button", { name: "Save public access" })
    )
    await waitFor(() => {
      expect(mocks.updateAppServicePublicAccess).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "app-resource-id",
        { enabled: true, rateLimitRpm: 120 }
      )
    })
    expect(onToast).toHaveBeenCalledWith(
      "Public hostname app-0123456789abcdef.knotree.org"
    )
  })

  it("shows injected HTML analytics on the dashboard", async () => {
    const user = userEvent.setup()
    mocks.getHtmlPageAnalytics.mockResolvedValue({
      pageviews: 12,
      sessions: 4,
      avgDurationMs: 1500,
      topPaths: [{ name: "/", count: 12 }],
      topReferrers: [{ name: "(direct)", count: 12 }],
      browsers: [{ name: "Chrome", count: 12 }],
      eventTypes: [{ name: "pageview", count: 12 }],
      recent: [
        {
          occurredAt: "2026-09-19T00:00:00Z",
          eventType: "pageview",
          path: "/",
          referrer: null,
          sessionId: "abc",
        },
      ],
    })
    render(
      <ResourceWorkspace
        node={{
          id: "html-page",
          title: "Docs",
          type: "HTML page",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "html-id",
            name: "Docs",
            resourceType: "app",
            status: "ready",
            image: "nginxinc/nginx-unprivileged:1.27-alpine",
            imageSource: "html",
            appPort: 8080,
            host: "localhost",
            port: 8080,
            serviceUrl: "https://page-docs.knotree.org",
            publicDomain: "page-docs.knotree.org",
            containerName: "knotree-app-html",
          },
        }}
        environment="production"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={vi.fn()}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    expect(
      within(dialog).queryByRole("tab", { name: "Database" })
    ).not.toBeInTheDocument()
    await user.click(within(dialog).getByRole("tab", { name: "Analytics" }))
    expect(await within(dialog).findByRole("heading", { name: "Page analytics" })).toBeInTheDocument()
    expect(within(dialog).getByText("Pageviews")).toBeInTheDocument()
    expect(within(dialog).getByText("Chrome")).toBeInTheDocument()
    expect(mocks.getHtmlPageAnalytics).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "test-2",
      "html-id"
    )
  })

  it("edits pasted HTML from settings and redeploys", async () => {
    const user = userEvent.setup()
    const onToast = vi.fn()
    mocks.getHtmlPageIndex.mockResolvedValue({
      indexHtml: "<html><body>hello</body></html>",
    })
    mocks.updateHtmlPage.mockResolvedValue({
      id: "html-id",
      name: "Docs",
      resourceType: "app",
      status: "provisioning",
      image: "nginxinc/nginx-unprivileged:1.27-alpine",
      imageSource: "html",
      appPort: 8080,
      host: "localhost",
      port: 8080,
      serviceUrl: "https://page-docs.knotree.org",
      publicDomain: "page-docs.knotree.org",
      containerName: "knotree-app-html",
    })
    render(
      <ResourceWorkspace
        node={{
          id: "html-page",
          title: "Docs",
          type: "HTML page",
          volume: "app-service",
          status: "ACTIVE",
          resource: {
            id: "html-id",
            name: "Docs",
            resourceType: "app",
            status: "ready",
            image: "nginxinc/nginx-unprivileged:1.27-alpine",
            imageSource: "html",
            appPort: 8080,
            host: "localhost",
            port: 8080,
            serviceUrl: "https://page-docs.knotree.org",
            publicDomain: "page-docs.knotree.org",
            containerName: "knotree-app-html",
          },
        }}
        environment="production"
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="test-2"
        onClose={vi.fn()}
        onCopyConnectionString={vi.fn()}
        copiedConnectionString={false}
        onToast={onToast}
        onOpenLogs={vi.fn()}
      />
    )

    const dialog = screen.getByRole("dialog")
    await user.click(within(dialog).getByRole("tab", { name: "Settings" }))
    const editor = await within(dialog).findByLabelText("index.html")
    await user.clear(editor)
    await user.type(editor, "<html><body>updated-v2</body></html>")
    await user.click(
      within(dialog).getByRole("button", { name: "Save and deploy" })
    )
    await waitFor(() => {
      expect(mocks.updateHtmlPage).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "test-2",
        "html-id",
        { indexHtml: "<html><body>updated-v2</body></html>" }
      )
    })
    expect(onToast).toHaveBeenCalledWith("HTML page redeploy started.")
  })
})
