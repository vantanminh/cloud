import { render, screen } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { ImageStoreWorkspace } from "@/components/image-store-workspace"
import type { ImageStore } from "@/lib/types"

const mocks = vi.hoisted(() => ({
  listImageKeys: vi.fn(),
  listImageObjects: vi.fn(),
  createImageKey: vi.fn(),
  revokeImageKey: vi.fn(),
  uploadImageObject: vi.fn(),
  deleteImageObject: vi.fn(),
  signImageObject: vi.fn(),
}))

vi.mock("@/lib/resources", () => ({
  listImageKeys: mocks.listImageKeys,
  listImageObjects: mocks.listImageObjects,
  createImageKey: mocks.createImageKey,
  revokeImageKey: mocks.revokeImageKey,
  uploadImageObject: mocks.uploadImageObject,
  deleteImageObject: mocks.deleteImageObject,
  signImageObject: mocks.signImageObject,
}))

const store: ImageStore = {
  id: "store-id",
  name: "Website images",
  resourceType: "images",
  compressionMode: "per_url",
  maxWidth: 1600,
  maxHeight: null,
  quality: 80,
  objectCount: 0,
  byteSize: 0,
  publicBaseUrl: "https://img.knotree.org",
}

describe("ImageStoreWorkspace", () => {
  beforeEach(() => {
    mocks.listImageKeys.mockResolvedValue([])
    mocks.listImageObjects.mockResolvedValue({ objects: [], folders: [] })
    mocks.createImageKey.mockResolvedValue({
      id: "key-id",
      name: "Browser client",
      clientId: "kimg_public",
      clientSecret: "ksec_once",
      access: "browser",
      status: "active",
      createdAt: "2026-10-02T00:00:00Z",
      revokedAt: null,
    })
    mocks.revokeImageKey.mockResolvedValue({
      id: "key-id",
      name: "Browser client",
      clientId: "kimg_public",
      access: "browser",
      status: "revoked",
      createdAt: "2026-10-02T00:00:00Z",
      revokedAt: "2026-10-02T01:00:00Z",
    })
  })

  it("creates a browser key, shows the secret once, and revokes it", async () => {
    const user = userEvent.setup()
    render(
      <ImageStoreWorkspace
        store={store}
        workspaceId="workspace-id"
        projectSlug="knotree-study"
        onClose={() => undefined}
        onChanged={() => undefined}
      />
    )

    expect(
      await screen.findByRole("heading", { name: "Website images" })
    ).toBeInTheDocument()
    expect(screen.getByText(/cached for one year/i)).toBeInTheDocument()
    expect(screen.getByText(/own WebP size/i)).toBeInTheDocument()

    await user.click(screen.getByRole("button", { name: "Create key" }))
    expect(await screen.findByText(/ksec_once/)).toBeInTheDocument()
    expect(screen.getAllByText(/kimg_public/).length).toBeGreaterThan(0)

    await user.click(
      screen.getByRole("button", { name: "Revoke Browser client" })
    )
    expect(await screen.findByText(/revoked/)).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Revoke Browser client" })
    ).not.toBeInTheDocument()
  })
})
