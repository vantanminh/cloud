import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { AppServiceCreateDialog } from "@/components/app-service-create-dialog"

const mocks = vi.hoisted(() => ({
  createAppService: vi.fn(),
  getGithubConnectionStatus: vi.fn(),
  getGithubAuthorizationUrl: vi.fn(),
  listAppServices: vi.fn(),
}))

vi.mock("@/lib/resources", () => ({
  createAppService: mocks.createAppService,
  getGithubConnectionStatus: mocks.getGithubConnectionStatus,
  getGithubAuthorizationUrl: mocks.getGithubAuthorizationUrl,
  listAppServices: mocks.listAppServices,
  appServiceDeploymentEventsUrl: () => "/events",
}))

describe("AppServiceCreateDialog HTML pages", () => {
  beforeEach(() => {
    vi.clearAllMocks()
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
    })
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
    })
  })
})
