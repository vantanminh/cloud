import { render, screen, waitFor, within } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { RegistryConnectionPanel } from "@/components/registry-connection-panel"
import type { AppService } from "@/lib/types"

const mocks = vi.hoisted(() => ({
  attachAppServiceRegistryConnection: vi.fn(),
  createKnotreeRegistryConnection: vi.fn(),
  deleteKnotreeRegistryConnection: vi.fn(),
  listAppServices: vi.fn(),
  listKnotreeRegistryConnections: vi.fn(),
  listRegistryDeploys: vi.fn(),
  startKnotreeRegistryConsent: vi.fn(),
  updateAppServiceAutoDeploy: vi.fn(),
  updateKnotreeRegistryConnection: vi.fn(),
}))

vi.mock("@/lib/resources", () => mocks)

const WORKSPACE = "de305d54-75b4-431b-adb2-eb6b9e546014"
const DIGEST = `sha256:${"a".repeat(64)}`

const service: AppService = {
  id: "app-1",
  name: "API",
  resourceType: "app",
  status: "ready",
  image: "registry.knotree.com/team/api:production",
  imageSource: "knotree_registry",
  appPort: 8080,
  host: "localhost",
  port: 59601,
  serviceUrl: null,
  containerName: "knotree-app-1",
  autoDeployEnabled: true,
  deployedImageDigest: DIGEST,
  autoDeployCheckedAt: "2026-10-01T09:00:00Z",
  registryConnectionId: "connection-1",
}

function renderPanel(
  appService: AppService,
  handlers: {
    onToast?: (message: string) => void
    onAppServiceUpdated?: (resource: AppService) => void
  } = {}
) {
  return render(
    <RegistryConnectionPanel
      appService={appService}
      workspaceId={WORKSPACE}
      projectSlug="demo"
      onToast={handlers.onToast ?? vi.fn()}
      onAppServiceUpdated={handlers.onAppServiceUpdated}
    />
  )
}

describe("RegistryConnectionPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.listRegistryDeploys.mockResolvedValue({
      appServiceId: "app-1",
      autoDeployReady: true,
      jobs: [],
    })
    mocks.listKnotreeRegistryConnections.mockResolvedValue({
      connections: [],
      autoDeployReady: true,
    })
  })

  it("shows a live connection with the push-to-deploy flow and recent pushes", async () => {
    mocks.listRegistryDeploys.mockResolvedValue({
      appServiceId: "app-1",
      autoDeployReady: true,
      jobs: [
        {
          id: "job-3",
          imageDigest: `sha256:${"c".repeat(64)}`,
          status: "failed",
          attemptCount: 1,
          lastError: "Registry image deployment failed.",
          deploymentId: "deployment-3",
          receivedAt: "2026-10-01T09:30:00Z",
          updatedAt: "2026-10-01T09:31:00Z",
        },
        {
          id: "job-2",
          imageDigest: `sha256:${"b".repeat(64)}`,
          status: "succeeded",
          attemptCount: 0,
          lastError: null,
          deploymentId: null,
          receivedAt: "2026-10-01T09:20:00Z",
          updatedAt: "2026-10-01T09:20:00Z",
        },
        {
          id: "job-1",
          imageDigest: DIGEST,
          status: "succeeded",
          attemptCount: 1,
          lastError: null,
          deploymentId: "deployment-1",
          receivedAt: "2026-10-01T09:00:00Z",
          updatedAt: "2026-10-01T09:02:00Z",
        },
      ],
    })

    renderPanel(service)

    const panel = screen.getByRole("region", {
      name: "Knotree Registry connection",
    })
    expect(within(panel).getByText("Live")).toBeInTheDocument()
    expect(
      within(panel).getByText(
        "Every push to the production tag deploys its exact image digest automatically."
      )
    ).toBeInTheDocument()
    expect(
      within(panel).getByRole("list", { name: "How auto deploy works" })
    ).toHaveTextContent("You push a new image to the production tag.")
    expect(mocks.listRegistryDeploys).toHaveBeenCalledWith(
      WORKSPACE,
      "demo",
      "app-1"
    )
    expect(await within(panel).findByText("Failed")).toBeInTheDocument()
    expect(within(panel).getByText("Skipped")).toBeInTheDocument()
    expect(within(panel).getByText("Deployed")).toBeInTheDocument()
    expect(
      within(panel).getByText("Registry image deployment failed.")
    ).toBeInTheDocument()
    expect(
      within(panel).getByText(`sha256:${"c".repeat(12)}`)
    ).toBeInTheDocument()
    expect(
      within(panel).queryByRole("heading", { name: "Reconnect" })
    ).not.toBeInTheDocument()
  })

  it("marks auto deploy unavailable when Cloud has no webhook secret", async () => {
    mocks.listRegistryDeploys.mockResolvedValue({
      appServiceId: "app-1",
      autoDeployReady: false,
      jobs: [],
    })
    renderPanel({ ...service, autoDeployEnabled: false })

    expect(await screen.findByText("Unavailable")).toBeInTheDocument()
    expect(
      screen.getByRole("checkbox", {
        name: "Auto deploy new Knotree Registry images",
      })
    ).toBeDisabled()
    expect(screen.getByText(/No pushes received yet/)).toBeInTheDocument()
  })

  it("reconnects a disconnected service to a saved connection without recreating it", async () => {
    const user = userEvent.setup()
    const onToast = vi.fn()
    const onAppServiceUpdated = vi.fn()
    const disconnected = {
      ...service,
      autoDeployEnabled: false,
      registryConnectionId: null,
    }
    mocks.listKnotreeRegistryConnections.mockResolvedValue({
      connections: [
        {
          id: "other-repo",
          registryHost: "registry.knotree.com",
          username: "deployer",
          repository: "team/web",
          verifiedAt: "2026-10-01T08:00:00Z",
        },
        {
          id: "connection-2",
          registryHost: "registry.knotree.com",
          username: "deployer",
          repository: "team/api",
          verifiedAt: "2026-10-01T08:00:00Z",
        },
      ],
      autoDeployReady: true,
    })
    mocks.attachAppServiceRegistryConnection.mockResolvedValue({
      ...disconnected,
      registryConnectionId: "connection-2",
    })

    renderPanel(disconnected, { onToast, onAppServiceUpdated })

    expect(screen.getByText("Disconnected")).toBeInTheDocument()
    expect(
      screen.queryByRole("checkbox", {
        name: "Auto deploy new Knotree Registry images",
      })
    ).not.toBeInTheDocument()
    const choice = await screen.findByLabelText("Saved connection")
    expect(within(choice).getAllByRole("option")).toHaveLength(1)
    await user.click(screen.getByRole("button", { name: "Reconnect" }))

    await waitFor(() => {
      expect(mocks.attachAppServiceRegistryConnection).toHaveBeenCalledWith(
        WORKSPACE,
        "demo",
        "app-1",
        "connection-2"
      )
    })
    expect(onAppServiceUpdated).toHaveBeenCalledWith(
      expect.objectContaining({ registryConnectionId: "connection-2" })
    )
    expect(onToast).toHaveBeenCalledWith(
      "Knotree Registry reconnected. Turn on auto deploy to deploy new pushes automatically."
    )
  })

  it("verifies a new pull token for the service repository and attaches it", async () => {
    const user = userEvent.setup()
    mocks.createKnotreeRegistryConnection.mockResolvedValue({
      id: "connection-3",
      registryHost: "registry.knotree.com",
      username: "deployer",
      repository: "team/api",
      verifiedAt: "2026-10-01T08:00:00Z",
    })
    mocks.attachAppServiceRegistryConnection.mockResolvedValue({
      ...service,
      registryConnectionId: "connection-3",
    })

    renderPanel({ ...service, registryConnectionId: null })

    await user.type(screen.getByLabelText("Registry username"), "deployer")
    await user.type(screen.getByLabelText("Pull-only token"), "pull-pat")
    await user.click(screen.getByRole("button", { name: "Connect with token" }))

    await waitFor(() => {
      expect(mocks.createKnotreeRegistryConnection).toHaveBeenCalledWith(
        WORKSPACE,
        "demo",
        { username: "deployer", token: "pull-pat", repository: "team/api" }
      )
      expect(mocks.attachAppServiceRegistryConnection).toHaveBeenCalledWith(
        WORKSPACE,
        "demo",
        "app-1",
        "connection-3"
      )
    })
  })
})
