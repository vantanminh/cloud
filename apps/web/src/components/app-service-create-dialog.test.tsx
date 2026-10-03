import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { AppServiceCreateDialog } from "@/components/app-service-create-dialog"

const mocks = vi.hoisted(() => ({
  createAppService: vi.fn(),
  getGithubConnectionStatus: vi.fn(),
  getGithubAuthorizationUrl: vi.fn(),
  listAppServices: vi.fn(),
  getKnotreeRegistryAccount: vi.fn(),
  listKnotreeRegistryRepositories: vi.fn(),
  listKnotreeRegistryTags: vi.fn(),
  importKnotreeRegistryRepository: vi.fn(),
}))

vi.mock("@/lib/resources", () => ({
  createAppService: mocks.createAppService,
  getGithubConnectionStatus: mocks.getGithubConnectionStatus,
  getGithubAuthorizationUrl: mocks.getGithubAuthorizationUrl,
  listAppServices: mocks.listAppServices,
  appServiceDeploymentEventsUrl: () => "/events",
  getKnotreeRegistryAccount: mocks.getKnotreeRegistryAccount,
  listKnotreeRegistryRepositories: mocks.listKnotreeRegistryRepositories,
  listKnotreeRegistryTags: mocks.listKnotreeRegistryTags,
  importKnotreeRegistryRepository: mocks.importKnotreeRegistryRepository,
}))

describe("AppServiceCreateDialog HTML pages", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.getKnotreeRegistryAccount.mockResolvedValue({
      connected: true,
      autoDeployReady: false,
      namespace: "kt-owner",
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

  it("only deploys Knotree Registry images from the account's own namespace", async () => {
    const user = userEvent.setup()
    mocks.listKnotreeRegistryRepositories.mockResolvedValue({
      namespace: "kt-owner",
      registryHost: "registry.knotree.com",
      repositories: [],
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
    // Nothing to connect: no consent button and no token fields.
    expect(
      screen.queryByRole("button", { name: /Connect Knotree Registry/ })
    ).toBeNull()
    expect(screen.queryByLabelText("Pull-only access token")).toBeNull()
    await waitFor(() =>
      expect(mocks.getKnotreeRegistryAccount).toHaveBeenCalled()
    )
    await user.click(screen.getByRole("button", { name: "Deploy service" }))

    expect(
      await screen.findByText(
        "Choose an image from your namespace, registry.knotree.com/kt-owner/…."
      )
    ).toBeInTheDocument()
    expect(mocks.importKnotreeRegistryRepository).not.toHaveBeenCalled()
    expect(mocks.createAppService).not.toHaveBeenCalled()
  })

  it("imports an image from the account's Registry namespace with auto-deploy", async () => {
    const user = userEvent.setup()
    mocks.getKnotreeRegistryAccount.mockResolvedValue({
      connected: true,
      autoDeployReady: true,
      namespace: "kt-owner",
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
    // There is never a token to paste for the account's own images.
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
  })
})
