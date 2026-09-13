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
              id: "existing-workspace-id",
              name: "Existing Workspace",
              slug: "existing-workspace",
            },
          })
        }
        if (path === "/workspaces") {
          return Promise.resolve({
            id: "workspace-id",
            name: "Acme Studio",
            slug: "acme-studio",
          })
        }
        if (path === "/workspaces/acme-studio/projects") {
          if (options?.method === "POST") {
            return Promise.resolve({
              id: "project-id",
              name: "Knotree Study",
              slug: "knotree-study",
            })
          }
          return Promise.resolve([])
        }
        if (path === "/workspaces/acme-studio/projects/knotree-study") {
          return Promise.resolve({
            id: "project-id",
            name: "Knotree Study",
            slug: "knotree-study",
          })
        }
        if (
          path === "/workspaces/acme-studio/projects/knotree-study/resources"
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
        if (
          path ===
          "/workspaces/acme-studio/projects/knotree-study/resources/postgres-resource-id/database/tables"
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

  it("creates the first workspace and routes to its slug", async () => {
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
    expect(screen.getByLabelText("Workspace URL")).toHaveValue("acme-studio")
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

  it("creates a project and opens its topology home", async () => {
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
    expect(screen.getByLabelText("Project URL slug")).toHaveValue(
      "knotree-study"
    )
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Create project",
      })
    )

    expect(
      await screen.findByRole("heading", { name: "Knotree Study" })
    ).toBeInTheDocument()
    expect(screen.getByRole("heading", { name: "Knotree Study" }).tagName).toBe(
      "H2"
    )

    expect(
      await screen.findByText("Create your first database")
    ).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Add" }))
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
      await screen.findByRole("button", { name: "Postgres resource, online" })
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

    await user.click(screen.getByRole("button", { name: "Add" }))
    await user.click(screen.getByRole("button", { name: "Redis" }))
    expect(screen.getByRole("status")).toHaveTextContent(
      "Redis provisioning is coming soon"
    )

    await user.click(screen.getByRole("button", { name: "Logs" }))
    expect(screen.getByRole("heading", { name: /^Logs$/ })).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Topology" }))
    expect(
      screen.getByRole("heading", { name: "Knotree Study" })
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
