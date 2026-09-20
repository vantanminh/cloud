import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"
import { MemoryRouter, Route, Routes } from "react-router-dom"

import { AuthProvider } from "@/auth/auth-context"
import { GitHubIntegrationPage } from "@/pages/github-integration-page"

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

function renderPage(initialEntry = "/settings/integrations") {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <AuthProvider>
        <Routes>
          <Route
            path="/settings/integrations"
            element={<GitHubIntegrationPage />}
          />
        </Routes>
      </AuthProvider>
    </MemoryRouter>
  )
}

describe("GitHubIntegrationPage", () => {
  let connected = true

  beforeEach(() => {
    connected = true
    mocks.getCsrfToken.mockResolvedValue("csrf-token")
    mocks.apiRequest.mockImplementation(
      (path: string, options?: { method?: string }) => {
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
        if (path === "/auth/github/status") {
          return Promise.resolve({
            connected,
            login: connected ? "jane-doe" : null,
          })
        }
        if (path === "/auth/github/disconnect" && options?.method === "POST") {
          connected = false
          return Promise.resolve(undefined)
        }
        return Promise.reject(new Error(`Unexpected request: ${path}`))
      }
    )
  })

  it("shows the current user's GitHub connection and can disconnect it", async () => {
    const user = userEvent.setup()
    renderPage()

    expect(
      await screen.findByText("Connected as @jane-doe")
    ).toBeInTheDocument()
    await user.click(screen.getByRole("button", { name: "Disconnect" }))

    expect(
      await screen.findByText(
        "GitHub has been disconnected from your Knotree account."
      )
    ).toBeInTheDocument()
    expect(screen.getByText("Not connected")).toBeInTheDocument()
    expect(mocks.apiRequest).toHaveBeenCalledWith("/auth/github/disconnect", {
      method: "POST",
    })
  })

  it("explains a successful OAuth return on the account integration page", async () => {
    connected = false
    renderPage("/settings/integrations?github=connected")

    expect(
      await screen.findByText(
        "GitHub is connected. Private ghcr.io images can now be deployed from your projects."
      )
    ).toBeInTheDocument()
    expect(
      screen.getByRole("button", { name: "Connect GitHub" })
    ).toBeInTheDocument()
    expect(screen.getByText("jane@example.com")).toBeInTheDocument()
  })
})
