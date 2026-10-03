import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { AppServiceCreateDialog } from "@/components/app-service-create-dialog"

const mocks = vi.hoisted(() => ({
  createAppService: vi.fn(),
  createKnotreeRegistryConnection: vi.fn(),
  startKnotreeRegistryConsent: vi.fn(),
  listKnotreeRegistryConnections: vi.fn(),
  getGithubConnectionStatus: vi.fn(),
  getGithubAuthorizationUrl: vi.fn(),
  listAppServices: vi.fn(),
  getKnotreeRegistryAccount: vi.fn(),
  listKnotreeRegistryRepositories: vi.fn(),
  listKnotreeRegistryTags: vi.fn(),
  importKnotreeRegistryRepository: vi.fn(),
  startKnotreeRegistryAccountConsent: vi.fn(),
}))

vi.mock("@/lib/resources", () => ({
  createAppService: mocks.createAppService,
  createKnotreeRegistryConnection: mocks.createKnotreeRegistryConnection,
  startKnotreeRegistryConsent: mocks.startKnotreeRegistryConsent,
  getGithubConnectionStatus: mocks.getGithubConnectionStatus,
  getGithubAuthorizationUrl: mocks.getGithubAuthorizationUrl,
  listAppServices: mocks.listAppServices,
  listKnotreeRegistryConnections: mocks.listKnotreeRegistryConnections,
  appServiceDeploymentEventsUrl: () => "/events",
  getKnotreeRegistryAccount: mocks.getKnotreeRegistryAccount,
  listKnotreeRegistryRepositories: mocks.listKnotreeRegistryRepositories,
  listKnotreeRegistryTags: mocks.listKnotreeRegistryTags,
  importKnotreeRegistryRepository: mocks.importKnotreeRegistryRepository,
  startKnotreeRegistryAccountConsent: mocks.startKnotreeRegistryAccountConsent,
  isKnotreeRegistryAuthorizationUrl: (value: string) =>
    value.startsWith("https://registry.knotree.com/cloud/authorize/"),
}))

describe("AppServiceCreateDialog HTML pages", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.getKnotreeRegistryAccount.mockResolvedValue({
      connected: false,
      consentReady: true,
      autoDeployReady: false,
      namespace: null,
      expiresAt: null,
      expired: false,
    })
    mocks.createAppService.mockResolvedValue({
      id: "svc-1",
      name: "Docs",
      resourceType: "app",
      status: "provisioning",
      image: "nginxinc/nginx-unprivileged:1.27-alpine",
      imageSource: "html",
      appPort: 8080,
      host: null,
      port: null,
      serviceUrl: null,
      containerName: null,
    })
  })

  it("submits a pasted HTML page with a unique page- domain", async () => {
    const user = userEvent.setup()
    const onCreated = vi.fn()
    render(
      <AppServiceCreateDialog
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="proj"
        open
        onOpenChange={vi.fn()}
        onCreated={onCreated}
      />
    )

    await user.selectOptions(screen.getByLabelText("Service type"), "html")
    await user.clear(screen.getByLabelText("Service name"))
    await user.type(screen.getByLabelText("Service name"), "Docs")
    await user.type(screen.getByLabelText("Public domain"), "docs")
    await user.type(
      screen.getByLabelText("index.html"),
      "<html><body>hi</body></html>"
    )
    await user.click(screen.getByRole("button", { name: "Deploy service" }))

    expect(mocks.createAppService).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "proj",
      {
        name: "Docs",
        imageSource: "html",
        pageSlug: "docs",
        indexHtml: "<html><body>hi</body></html>",
        githubRepo: undefined,
        githubBranch: undefined,
        autoDeploy: false,
      }
    )
  })

  it("derives a unique page- suffix from the service name", async () => {
    const user = userEvent.setup()
    render(
      <AppServiceCreateDialog
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="proj"
        open
        onOpenChange={vi.fn()}
        onCreated={vi.fn()}
      />
    )
    await user.selectOptions(screen.getByLabelText("Service type"), "html")
    await user.clear(screen.getByLabelText("Service name"))
    await user.type(screen.getByLabelText("Service name"), "My Docs")
    await user.type(
      screen.getByLabelText("index.html"),
      "<html><body>hi</body></html>"
    )
    await user.click(screen.getByRole("button", { name: "Deploy service" }))
    expect(mocks.createAppService).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "proj",
      expect.objectContaining({
        name: "My Docs",
        imageSource: "html",
        pageSlug: "my-docs",
      })
    )
  })

  it("submits a GitHub HTML repository with auto-deploy", async () => {
    const user = userEvent.setup()
    mocks.getGithubConnectionStatus.mockResolvedValue({
      connected: true,
      login: "acme",
    })
    render(
      <AppServiceCreateDialog
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="proj"
        open
        onOpenChange={vi.fn()}
        onCreated={vi.fn()}
      />
    )
    await user.selectOptions(screen.getByLabelText("Service type"), "html")
    await user.selectOptions(screen.getByLabelText("HTML source"), "github")
    await user.clear(screen.getByLabelText("Service name"))
    await user.type(screen.getByLabelText("Service name"), "Docs")
    await user.type(screen.getByLabelText("Public domain"), "docs")
    await user.type(screen.getByLabelText("GitHub repository"), "acme/site")
    await user.click(screen.getByRole("button", { name: "Deploy service" }))
    expect(mocks.createAppService).toHaveBeenCalledWith(
      "de305d54-75b4-431b-adb2-eb6b9e546014",
      "proj",
      {
        name: "Docs",
        imageSource: "html_github",
        pageSlug: "docs",
        indexHtml: undefined,
        githubRepo: "acme/site",
        githubBranch: undefined,
        autoDeploy: true,
      }
    )
  })

  it("starts repository consent and rejects an external authorization destination", async () => {
    const user = userEvent.setup()
    mocks.listKnotreeRegistryConnections.mockResolvedValue({
      connections: [],
      autoDeployReady: false,
      consentReady: true,
    })
    mocks.startKnotreeRegistryConsent.mockResolvedValue({
      authorizationUrl: "https://attacker.example/cloud/authorize/123",
    })
    render(
      <AppServiceCreateDialog
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="proj"
        open
        onOpenChange={vi.fn()}
        onCreated={vi.fn()}
      />
    )
    await user.type(
      screen.getByLabelText("Docker image"),
      "registry.knotree.com/kt-owner/app:production"
    )
    await user.selectOptions(
      screen.getByLabelText("Image access"),
      "knotree_registry"
    )
    await user.click(
      await screen.findByRole("button", {
        name: "Authorize Registry pull access",
      })
    )
    await waitFor(() =>
      expect(mocks.startKnotreeRegistryConsent).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "proj",
        "kt-owner/app"
      )
    )
    expect(
      await screen.findByText("Registry returned an invalid authorization URL.")
    ).toBeInTheDocument()
    expect(mocks.createKnotreeRegistryConnection).not.toHaveBeenCalled()
  })

  it("connects a pull-only Knotree Registry token before creating a service", async () => {
    const user = userEvent.setup()
    mocks.listKnotreeRegistryConnections.mockResolvedValue({
      connections: [],
      autoDeployReady: false,
    })
    mocks.createKnotreeRegistryConnection.mockResolvedValue({
      id: "registry-connection-1",
      registryHost: "registry.knotree.com",
      username: "service-user",
      repository: "team/api",
      verifiedAt: "2026-09-23T09:00:00Z",
    })

    render(
      <AppServiceCreateDialog
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="proj"
        open
        onOpenChange={vi.fn()}
        onCreated={vi.fn()}
      />
    )

    await user.type(
      screen.getByLabelText("Docker image"),
      "registry.knotree.com/team/api:production"
    )
    await user.selectOptions(
      screen.getByLabelText("Image access"),
      "knotree_registry"
    )
    await waitFor(() => {
      expect(mocks.listKnotreeRegistryConnections).toHaveBeenCalled()
    })
    expect(
      screen.getByRole("checkbox", { name: /Auto-deploy new image digests/ })
    ).toBeDisabled()
    await user.type(screen.getByLabelText("Registry username"), "service-user")
    await user.type(screen.getByLabelText("Pull-only access token"), "pull-pat")
    expect(
      screen.getByText("repository:team/api:pull", { exact: false })
    ).toBeInTheDocument()
    await user.click(
      screen.getByRole("button", { name: "Connect Knotree Registry & deploy" })
    )

    await waitFor(() => {
      expect(mocks.createKnotreeRegistryConnection).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "proj",
        {
          username: "service-user",
          token: "pull-pat",
          repository: "team/api",
        }
      )
      expect(mocks.createAppService).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "proj",
        {
          name: "App service",
          image: "registry.knotree.com/team/api:production",
          imageSource: "knotree_registry",
          appPort: 3000,
          registryConnectionId: "registry-connection-1",
          autoDeploy: false,
        }
      )
    })
  })

  it("imports an image from the connected Registry account with auto-deploy", async () => {
    const user = userEvent.setup()
    mocks.getKnotreeRegistryAccount.mockResolvedValue({
      connected: true,
      consentReady: true,
      autoDeployReady: true,
      namespace: "kt-owner",
      expiresAt: "2026-11-01T00:00:00Z",
      expired: false,
    })
    mocks.listKnotreeRegistryConnections.mockResolvedValue({
      connections: [],
      autoDeployReady: true,
      consentReady: true,
    })
    mocks.listKnotreeRegistryRepositories.mockResolvedValue({
      namespace: "kt-owner",
      registryHost: "registry.knotree.com",
      repositories: [
        {
          name: "kt-owner/api",
          tagCount: 2,
          latestTag: "production",
          latestDigest: null,
          size: 0,
          updatedAt: null,
        },
      ],
    })
    mocks.listKnotreeRegistryTags.mockResolvedValue({
      repository: "kt-owner/api",
      tags: [
        {
          tag: "production",
          digest: `sha256:${"a".repeat(64)}`,
          size: 0,
          createdAt: null,
        },
        {
          tag: "latest",
          digest: `sha256:${"b".repeat(64)}`,
          size: 0,
          createdAt: null,
        },
      ],
    })
    mocks.importKnotreeRegistryRepository.mockResolvedValue({
      id: "account-connection-1",
      registryHost: "registry.knotree.com",
      username: "kt-owner",
      repository: "kt-owner/api",
      verifiedAt: "2026-10-03T00:00:00Z",
    })

    render(
      <AppServiceCreateDialog
        workspaceId="de305d54-75b4-431b-adb2-eb6b9e546014"
        projectSlug="proj"
        open
        onOpenChange={vi.fn()}
        onCreated={vi.fn()}
      />
    )
    await user.selectOptions(
      screen.getByLabelText("Image access"),
      "knotree_registry"
    )
    await user.click(
      await screen.findByRole("button", { name: /kt-owner\/api/ })
    )
    await waitFor(() =>
      expect(screen.getByLabelText("Docker image")).toHaveValue(
        "registry.knotree.com/kt-owner/api:production"
      )
    )
    await user.selectOptions(screen.getByLabelText("Tag"), "latest")
    expect(screen.getByLabelText("Docker image")).toHaveValue(
      "registry.knotree.com/kt-owner/api:latest"
    )
    // The per-repository token fields are not needed for the user's own images.
    expect(screen.queryByLabelText("Pull-only access token")).toBeNull()
    await user.click(
      screen.getByRole("checkbox", { name: /Auto-deploy new image digests/ })
    )
    await user.click(screen.getByRole("button", { name: "Deploy service" }))

    await waitFor(() => {
      expect(mocks.importKnotreeRegistryRepository).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "proj",
        "kt-owner/api"
      )
      expect(mocks.createAppService).toHaveBeenCalledWith(
        "de305d54-75b4-431b-adb2-eb6b9e546014",
        "proj",
        {
          name: "App service",
          image: "registry.knotree.com/kt-owner/api:latest",
          imageSource: "knotree_registry",
          appPort: 3000,
          registryConnectionId: "account-connection-1",
          autoDeploy: true,
        }
      )
    })
    expect(mocks.createKnotreeRegistryConnection).not.toHaveBeenCalled()
  })
})
