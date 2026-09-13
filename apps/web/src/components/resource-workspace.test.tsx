import { render, screen, within } from "@testing-library/react"
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
      retentionSeconds: 86400,
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
    expect(mocks.getDatabaseMetrics).toHaveBeenCalledWith(
      "mimo-i-tech",
      "test-2",
      "resource-id"
    )
  })
})
