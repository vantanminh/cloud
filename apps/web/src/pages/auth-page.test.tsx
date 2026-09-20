import { render, screen, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { MemoryRouter, Route, Routes } from "react-router-dom"

import { AuthProvider } from "@/auth/auth-context"
import App from "@/App"
import { AuthPage } from "@/pages/auth-page"

const mocks = vi.hoisted(() => {
  class MockApiError extends Error {
    readonly status: number
    readonly code: string
    readonly fields: Record<string, string>

    constructor(status: number, code: string, message: string) {
      super(message)
      this.name = "ApiError"
      this.status = status
      this.code = code
      this.fields = {}
    }
  }

  return {
    apiRequest: vi.fn(),
    getCsrfToken: vi.fn(),
    resetCsrfToken: vi.fn(),
    MockApiError,
  }
})

vi.mock("@/lib/api", () => ({
  ApiError: mocks.MockApiError,
  apiRequest: mocks.apiRequest,
  getCsrfToken: mocks.getCsrfToken,
  resetCsrfToken: mocks.resetCsrfToken,
}))

describe("AuthPage", () => {
  beforeEach(() => {
    mocks.getCsrfToken.mockResolvedValue("csrf-token")
    mocks.apiRequest.mockImplementation(
      (path: string, options?: { method?: string }) => {
        if (path === "/auth/me") {
          return Promise.reject(
            new mocks.MockApiError(
              401,
              "AUTHENTICATION_REQUIRED",
              "Authentication is required."
            )
          )
        }
        if (path === "/auth/register") {
          return Promise.resolve({
            user: {
              id: "user-id",
              fullName: "Jane Doe",
              email: "jane@example.com",
              emailVerified: false,
            },
            workspace: null,
          })
        }
        if (path === "/auth/login") {
          return Promise.resolve({
            user: {
              id: "existing-user-id",
              fullName: "Existing User",
              email: "existing@example.com",
              emailVerified: false,
            },
            workspace: {
              id: "6fa459ea-ee8a-3ca4-894e-db77e160355e",
              name: "Existing Workspace",
            },
          })
        }
        if (path === "/workspaces") {
          return Promise.resolve({
            id: "de305d54-75b4-431b-adb2-eb6b9e546014",
            name: "Acme Studio",
          })
        }
        if (path === "/workspaces/6fa459ea-ee8a-3ca4-894e-db77e160355e/projects") {
          return Promise.resolve([])
        }
        if (path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects") {
          if (options?.method === "POST") {
            return Promise.resolve({
              id: "project-id",
              name: "Knotree Study",
              slug: "knotree-study",
            })
          }
          return Promise.resolve([])
        }
        if (path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study") {
          return Promise.resolve({
            id: "project-id",
            name: "Knotree Study",
            slug: "knotree-study",
          })
        }
        if (
          path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/resources"
        ) {
          if (options?.method === "POST") {
            return Promise.resolve({
              id: "postgres-resource-id",
              name: "Postgres",
              resourceType: "postgres",
              status: "ready",
              databaseName: "knotree_db_project",
              username: "knotree_role_project",
              host: "localhost",
              port: 5432,
              connectionString:
                "postgres://knotree_role_project:secret@localhost:5432/knotree_db_project",
              clusterProvider: "docker",
              clusterName: "knotree-pg-project",
            })
          }
          return Promise.resolve([])
        }
        if (path === "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/redis") {
          if (options?.method === "POST") {
            return Promise.resolve({
              id: "redis-resource-id",
              name: "Redis",
              resourceType: "redis",
              status: "ready",
              host: "127.0.0.1",
              port: 6379,
              connectionString: "redis://:secret@127.0.0.1:6379",
              clusterProvider: "docker",
              clusterName: "knotree-redis-project",
              networkAlias: "redis",
              cpuLimit: "1",
              memoryLimit: "1Gi",
              storageLimit: "10Gi",
            })
          }
          return Promise.resolve([])
        }
        if (
          path ===
          "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/app-services"
        ) {
          if (options?.method === "POST") {
            return Promise.resolve({
              id: "app-service-id",
              name: "App service",
              resourceType: "app",
              status: "ready",
              image: "nginx:alpine",
              imageSource: "public",
              appPort: 80,
              host: "localhost",
              port: 32768,
              serviceUrl: "http://localhost:32768",
              containerName: "knotree-app-project",
            })
          }
          return Promise.resolve([])
        }
        if (
          path ===
          "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/resources/postgres-resource-id/database/tables"
        ) {
          return Promise.resolve([])
        }
        return Promise.reject(new Error(`Unexpected request: ${path}`))
      }
    )
  })

  it("registers a user and routes to first-workspace setup", async () => {
    const user = userEvent.setup()

    render(
      <MemoryRouter initialEntries={["/register"]}>
        <AuthProvider>
          <Routes>
            <Route path="/register" element={<AuthPage mode="register" />} />
            <Route path="/new/workspace" element={<p>workspace setup</p>} />
          </Routes>
        </AuthProvider>
      </MemoryRouter>
    )

    await user.type(await screen.findByLabelText("Full name"), "Jane Doe")
    await user.type(screen.getByLabelText("Work email"), "jane@example.com")
    await user.type(screen.getByLabelText("Password"), "correct horse")
    await user.click(screen.getByRole("button", { name: "Create account" }))

    expect(await screen.findByText("workspace setup")).toBeInTheDocument()
    expect(mocks.apiRequest).toHaveBeenCalledWith(
      "/auth/register",
      expect.objectContaining({ method: "POST" })
    )
  })

  it("creates the first workspace and routes to its UUID", async () => {
    const user = userEvent.setup()

    render(
      <MemoryRouter initialEntries={["/register"]}>
        <AuthProvider>
          <App />
        </AuthProvider>
      </MemoryRouter>
    )

    await user.type(await screen.findByLabelText("Full name"), "Jane Doe")
    await user.type(screen.getByLabelText("Work email"), "jane@example.com")
    await user.type(screen.getByLabelText("Password"), "correct horse")
    await user.click(screen.getByRole("button", { name: "Create account" }))

    await user.type(
      await screen.findByLabelText("Workspace name"),
      "Acme Studio"
    )
    expect(screen.queryByLabelText("Workspace URL")).not.toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Create workspace" }))

    await user.click(
      await screen.findByRole("button", { name: "Continue to workspace" })
    )
    expect(
      await screen.findByRole("heading", { name: "Welcome to Acme Studio" })
    ).toBeInTheDocument()
  })

  it("routes an existing user's login to its workspace", async () => {
    const user = userEvent.setup()

    render(
      <MemoryRouter initialEntries={["/login"]}>
        <AuthProvider>
          <App />
        </AuthProvider>
      </MemoryRouter>
    )

    await user.type(
      await screen.findByLabelText("Email"),
      "existing@example.com"
    )
    await user.type(screen.getByLabelText("Password"), "correct horse")
    await user.click(screen.getByRole("button", { name: "Sign in" }))

    expect(
      await screen.findByRole("heading", {
        name: "Welcome to Existing Workspace",
      })
    ).toBeInTheDocument()
  })

  it("creates a project, manages its resources, and opens topology", async () => {
    const user = userEvent.setup()
    const clipboardWrite = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: clipboardWrite },
    })

    render(
      <MemoryRouter initialEntries={["/register"]}>
        <AuthProvider>
          <App />
        </AuthProvider>
      </MemoryRouter>
    )

    await user.type(await screen.findByLabelText("Full name"), "Jane Doe")
    await user.type(screen.getByLabelText("Work email"), "jane@example.com")
    await user.type(screen.getByLabelText("Password"), "correct horse")
    await user.click(screen.getByRole("button", { name: "Create account" }))

    await user.type(
      await screen.findByLabelText("Workspace name"),
      "Acme Studio"
    )
    await user.click(screen.getByRole("button", { name: "Create workspace" }))
    await user.click(
      await screen.findByRole("button", { name: "Continue to workspace" })
    )

    await screen.findByRole("heading", { name: "Create your first project" })
    await user.click(screen.getAllByRole("button", { name: "New project" })[0])
    await user.type(
      await screen.findByLabelText("Project name"),
      "Knotree Study"
    )
    expect(screen.getByText("/knotree-study")).toBeInTheDocument()
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Create project",
      })
    )

    expect(
      await screen.findByRole("heading", { name: "Resources" })
    ).toBeInTheDocument()
    expect(
      screen.getByText("Knotree Study", {
        selector: ".project-project-context",
      })
    ).toBeInTheDocument()
    expect(
      await screen.findByRole("heading", { name: "No resources yet" })
    ).toBeInTheDocument()
    await user.click(screen.getAllByRole("button", { name: "Add resource" })[0])
    await user.click(screen.getByRole("button", { name: /^Postgres$/ }))
    const databaseName = await screen.findByLabelText("Database name")
    await user.clear(databaseName)
    await user.type(databaseName, "Analytics")
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Create database",
      })
    )

    const resourceDialog = await screen.findByRole("dialog")
    expect(
      within(resourceDialog).getByRole("tab", { name: "Deployments" })
    ).toHaveAttribute("aria-selected", "true")
    expect(screen.getAllByText("knotree_db_project").length).toBeGreaterThan(0)
    await user.click(
      within(resourceDialog).getByRole("tab", { name: "Database" })
    )
    expect(await screen.findByText("No tables yet")).toBeInTheDocument()
    await user.click(
      within(resourceDialog).getByRole("button", { name: "Connect" })
    )
    expect(
      await screen.findByText("Connection string copied")
    ).toBeInTheDocument()
    expect(clipboardWrite).toHaveBeenCalledWith(
      "postgres://knotree_role_project:secret@localhost:5432/knotree_db_project"
    )
    await user.click(
      within(resourceDialog).getByRole("button", {
        name: "Close resource workspace",
      })
    )
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument()

    await user.click(
      await screen.findByRole("button", { name: "Open Postgres" })
    )
    const reopenedResourceDialog = await screen.findByRole("dialog")
    await user.click(
      within(reopenedResourceDialog).getByRole("tab", { name: "Database" })
    )
    expect(
      within(reopenedResourceDialog).getByRole("button", { name: "Copied" })
    ).toBeInTheDocument()
    await user.click(
      within(reopenedResourceDialog).getByRole("button", {
        name: "Close resource workspace",
      })
    )

    await user.click(screen.getByRole("button", { name: "Add resource" }))
    await user.click(screen.getByRole("button", { name: /^Redis$/ }))
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Create Redis",
      })
    )
    expect(mocks.apiRequest).toHaveBeenCalledWith(
      "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/redis",
      expect.objectContaining({ method: "POST" })
    )
    const redisWorkspace = await screen.findByRole("dialog", { name: "Redis" })
    expect(
      within(redisWorkspace).getByRole("heading", { name: "Redis" })
    ).toBeInTheDocument()
    await user.click(
      within(redisWorkspace).getByRole("button", {
        name: "Close resource workspace",
      })
    )
    expect(
      await screen.findByRole("button", { name: "Open Redis" })
    ).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Add resource" }))
    await user.click(
      screen.getByRole("button", { name: "App service / HTML page" })
    )
    await user.selectOptions(
      await screen.findByLabelText("Service type"),
      "docker"
    )
    await user.type(
      await screen.findByLabelText("Docker image"),
      "nginx:alpine"
    )
    await user.clear(screen.getByLabelText("Container port"))
    await user.type(screen.getByLabelText("Container port"), "80")
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Deploy service",
      })
    )
    const appResourceDialog = await screen.findByRole("dialog")
    expect(
      within(appResourceDialog).getByText("Public · nginx:alpine")
    ).toBeInTheDocument()
    expect(
      within(appResourceDialog).getByText("Service URL · http://localhost:32768")
    ).toBeInTheDocument()
    expect(mocks.apiRequest).toHaveBeenCalledWith(
      "/workspaces/de305d54-75b4-431b-adb2-eb6b9e546014/projects/knotree-study/app-services",
      expect.objectContaining({ method: "POST" })
    )
    await user.click(
      within(appResourceDialog).getByRole("button", {
        name: "Close resource workspace",
      })
    )

    await user.click(screen.getByRole("button", { name: "Logs" }))
    expect(screen.getByRole("heading", { name: /^Logs$/ })).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Topology" }))
    expect(
      screen.getByRole("region", {
        name: "production infrastructure topology for Knotree Study",
      })
    ).toBeInTheDocument()

    await user.click(
      screen.getByRole("button", { name: "Switch to dark mode" })
    )
    expect(
      screen.getByRole("button", { name: "Switch to light mode" })
    ).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Zoom in" }))
    expect(screen.getByText("110%")).toBeInTheDocument()
  })
})
