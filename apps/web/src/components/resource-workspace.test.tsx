import { fireEvent, render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { ResourceWorkspace } from "@/components/resource-workspace"

const mocks = vi.hoisted(() => ({
  createDatabaseTable: vi.fn(),
  executeDatabaseQuery: vi.fn(),
  getDatabaseConfig: vi.fn(),
  getDatabaseMetrics: vi.fn(),
  getDatabaseStats: vi.fn(),
  getDatabaseTableData: vi.fn(),
  listDatabaseTables: vi.fn(),
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

  it("shows the automatically assigned private database connection", async () => {
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
      within(dialog).getByText("Postgres connected automatically")
    ).toBeInTheDocument()
    expect(within(dialog).getByText("postgres:5432")).toBeInTheDocument()

    await user.click(within(dialog).getByRole("tab", { name: "Variables" }))
    expect(await within(dialog).findByText("PGDATABASE")).toBeInTheDocument()
    expect(within(dialog).getByText("knotree_db_project")).toBeInTheDocument()
  })
})
