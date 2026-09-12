import { render, screen } from "@testing-library/react"
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
    mocks.apiRequest.mockImplementation((path: string) => {
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
      return Promise.reject(new Error(`Unexpected request: ${path}`))
    })
  })

  it("registers a user and routes to first-workspace setup", async () => {
    const user = userEvent.setup()

    render(
      <MemoryRouter initialEntries={["/register"]}>
        <AuthProvider>
          <Routes>
            <Route path="/register" element={<AuthPage mode="register" />} />
            <Route
              path="/new/workspace"
              element={<p>workspace setup</p>}
            />
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

    await user.type(await screen.findByLabelText("Workspace name"), "Acme Studio")
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

    await user.type(await screen.findByLabelText("Email"), "existing@example.com")
    await user.type(screen.getByLabelText("Password"), "correct horse")
    await user.click(screen.getByRole("button", { name: "Sign in" }))

    expect(
      await screen.findByRole("heading", { name: "Welcome to Existing Workspace" })
    ).toBeInTheDocument()
  })
})
