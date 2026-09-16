import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { RedisCreateDialog } from "@/components/redis-create-dialog"

const mocks = vi.hoisted(() => ({
  createRedisResource: vi.fn(),
}))

vi.mock("@/lib/resources", () => ({
  createRedisResource: mocks.createRedisResource,
}))

const createdRedis = {
  id: "redis-resource-id",
  name: "Cache",
  resourceType: "redis" as const,
  status: "ready" as const,
  host: "knotree-redis-11111111222233334444555555555555.knotree-cloud.svc.cluster.local",
  port: 6379,
  connectionString: "redis://:secret@knotree-redis-11111111222233334444555555555555.knotree-cloud.svc.cluster.local:6379",
  clusterProvider: "kubernetes" as const,
  clusterName: "knotree-redis-11111111222233334444555555555555",
  networkAlias: "knotree-redis-11111111222233334444555555555555",
  cpuLimit: "1",
  memoryLimit: "1Gi",
  storageLimit: "10Gi",
}

describe("RedisCreateDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.createRedisResource.mockResolvedValue(createdRedis)
  })

  it("creates Redis through the real API path instead of a coming-soon stub", async () => {
    const user = userEvent.setup()
    const onCreated = vi.fn()
    render(
      <RedisCreateDialog
        workspaceSlug="mimo-i-tech"
        projectSlug="test-2"
        open
        onOpenChange={vi.fn()}
        onCreated={onCreated}
      />
    )

    expect(screen.queryByText(/coming soon/i)).not.toBeInTheDocument()

    const name = await screen.findByLabelText("Redis name")
    await user.clear(name)
    await user.type(name, "Cache")
    await user.click(screen.getByRole("button", { name: "Create Redis" }))

    await waitFor(() => {
      expect(mocks.createRedisResource).toHaveBeenCalledWith(
        "mimo-i-tech",
        "test-2",
        { name: "Cache" }
      )
    })
    expect(onCreated).toHaveBeenCalledWith(createdRedis)
  })
})
