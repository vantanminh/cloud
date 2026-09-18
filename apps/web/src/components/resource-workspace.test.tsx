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
  listDatabaseTables: vi.fn(),
  listPostgresResources: vi.fn(),
  updateAppService: vi.fn(),
  updateAppServiceAutoDeploy: vi.fn(),
  updateAppServiceDatabase: vi.fn(),
  updateAppServicePublicAccess: vi.fn(),
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
        workspaceSlug="mimo-i-tech"
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
      "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
      "mimo-i-tech",
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
        "mimo-i-tech",
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
        "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
      "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
        workspaceSlug="mimo-i-tech"
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
      "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
        "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
        "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
        workspaceSlug="mimo-i-tech"
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
        "mimo-i-tech",
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
        workspaceSlug="mimo-i-tech"
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
        "mimo-i-tech",
        "test-2",
        "app-resource-id",
        { enabled: true, rateLimitRpm: 120 }
      )
    })
    expect(onToast).toHaveBeenCalledWith(
      "Public hostname app-0123456789abcdef.knotree.org"
    )
  })
})
