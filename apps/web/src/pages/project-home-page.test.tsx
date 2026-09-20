import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { MemoryRouter } from "react-router-dom"

import { AuthProvider } from "@/auth/auth-context"
import App from "@/App"

const mocks = vi.hoisted(() => ({
  apiRequest: vi.fn(),
  getCsrfToken: vi.fn(),
  resetCsrfToken: vi.fn(),
}))

vi.mock("@/lib/api", () => ({
  ApiError: class MockApiError extends Error {
    readonly status = 500
    readonly code = "REQUEST_FAILED"
    readonly fields = {}
  },
  apiRequest: mocks.apiRequest,
  getCsrfToken: mocks.getCsrfToken,
  resetCsrfToken: mocks.resetCsrfToken,
}))

function renderPage() {
  return render(
    <MemoryRouter
      initialEntries={["/workspace/de305d54-75b4-431b-adb2-eb6b9e546014/project/knotree-study"]}
    >
      <AuthProvider>
        <App />
      </AuthProvider>
    </MemoryRouter>
  )
}

describe("ProjectHomePage", () => {
  beforeEach(() => {
    window.localStorage.clear()
    mocks.getCsrfToken.mockResolvedValue("csrf-token")
    mocks.apiRequest.mockImplementation((path: string) => {
      if (path === "/auth/me") {
        return Promise.resolve({
          user: {
            id: "user-id",
            fullName: "Jane Doe",
            email: "jane@example.com",
            emailVerified: true,
          },
          workspace: {
            id: "de305d54-75b4-431b-adb2-eb6b9e546014",
            name: "Acme Studio",
          },
        })
      }
      if (path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study") {
        return Promise.resolve({
          id: "project-id",
          name: "Knotree Study",
          slug: "knotree-study",
        })
      }
      if (path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/resources") {
        return Promise.resolve([
          {
            id: "postgres-id",
            name: "primary",
            resourceType: "postgres",
            status: "ready",
            databaseName: "knotree_study",
            username: "knotree_user",
            host: "postgres.internal",
            port: 5432,
            connectionString: null,
            clusterProvider: "kubernetes",
            clusterName: "project-db",
          },
        ])
      }
      if (path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/redis") {
        return Promise.resolve([
          {
            id: "redis-id",
            name: "cache",
            resourceType: "redis",
            status: "ready",
            clusterName: "project-cache",
            networkAlias: "cache",
            host: "redis.internal",
            port: 6379,
            connectionString: null,
          },
        ])
      }
      if (
        path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/app-services"
      ) {
        return Promise.resolve([
          {
            id: "api-id",
            name: "api",
            resourceType: "app",
            status: "ready",
            image: "nginx:latest",
            imageSource: "public",
            appPort: 80,
            host: null,
            port: null,
            serviceUrl: null,
            publicDomain: null,
            containerName: "api",
            databaseConnection: null,
            deployment: null,
          },
          {
            id: "worker-id",
            name: "worker",
            resourceType: "app",
            status: "provisioning",
            image: "worker:latest",
            imageSource: "public",
            appPort: 8080,
            host: null,
            port: null,
            serviceUrl: null,
            publicDomain: null,
            containerName: "worker",
            databaseConnection: null,
            deployment: null,
          },
        ])
      }
      if (path.includes("/app-services/api-id/metrics")) {
        return Promise.resolve({
          points: [],
          systemMetricsAvailable: true,
          retentionSeconds: 2_592_000,
          sampleIntervalSeconds: 5,
          resolutionSeconds: 5,
          provider: "kubernetes",
          fromTimestamp: 0,
          toTimestamp: 0,
        })
      }
      return Promise.reject(new Error(`Unexpected request: ${path}`))
    })
  })

  it("lists multiple resource types, filters them, and opens a resource", async () => {
    const user = userEvent.setup()
    renderPage()

    expect(
      await screen.findByRole("heading", { name: "Resources" })
    ).toBeInTheDocument()
    await screen.findByRole("table")
    expect(screen.getAllByText("Knotree Study").length).toBeGreaterThan(0)
    expect(screen.getByText("primary")).toBeInTheDocument()
    expect(screen.getByText("cache")).toBeInTheDocument()
    expect(screen.getByText("api")).toBeInTheDocument()
    expect(screen.getByText("worker")).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Add resource" }))
    expect(
      screen.getByRole("button", { name: "App service / HTML page" })
    ).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "App services" }))
    expect(screen.getByText("api")).toBeInTheDocument()
    expect(screen.getByText("worker")).toBeInTheDocument()
    expect(screen.queryByText("primary")).not.toBeInTheDocument()

    await user.type(
      screen.getByRole("searchbox", { name: "Search resources" }),
      "worker"
    )
    expect(screen.getByText("worker")).toBeInTheDocument()
    expect(screen.queryByText("api")).not.toBeInTheDocument()

    await user.clear(
      screen.getByRole("searchbox", { name: "Search resources" })
    )
    await user.click(screen.getByRole("button", { name: "Open api" }))
    const dialog = await screen.findByRole("dialog")
    expect(
      within(dialog).getByRole("heading", { name: "api" })
    ).toBeInTheDocument()
  })

  it("opens topology, metrics, and logs from the project navigation", async () => {
    const user = userEvent.setup()
    renderPage()

    await screen.findByRole("heading", { name: "Resources" })
    await user.click(screen.getByRole("button", { name: "Topology" }))
    expect(
      screen.getByRole("region", {
        name: "production infrastructure topology for Knotree Study",
      })
    ).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Metrics" }))
    expect(
      await screen.findByRole("heading", { name: "Metrics" })
    ).toBeInTheDocument()
    await user.click(
      screen.getByRole("button", { name: "View metrics for api" })
    )
    expect(await screen.findByText("Runtime metrics")).toBeInTheDocument()

    await user.click(
      screen.getByRole("button", { name: "Close resource workspace" })
    )
    await user.click(screen.getByRole("button", { name: "Logs" }))
    expect(
      await screen.findByRole("heading", { name: "Logs" })
    ).toBeInTheDocument()
  })
})
