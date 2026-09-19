import { render, screen, waitFor } from "@testing-library/react"
import userEvent from "@testing-library/user-event"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { HtmlSourceEditor } from "@/components/html-page-workspace"
import type { AppService } from "@/lib/types"

const mocks = vi.hoisted(() => ({
  getHtmlPageIndex: vi.fn(),
  updateHtmlPage: vi.fn(),
  getHtmlPageAnalytics: vi.fn(),
}))

vi.mock("@/lib/resources", () => ({
  getHtmlPageIndex: mocks.getHtmlPageIndex,
  updateHtmlPage: mocks.updateHtmlPage,
  getHtmlPageAnalytics: mocks.getHtmlPageAnalytics,
}))

const pastedPage: AppService = {
  id: "html-id",
  name: "Docs",
  resourceType: "app",
  status: "ready",
  image: "nginxinc/nginx-unprivileged:1.27-alpine",
  imageSource: "html",
  appPort: 8080,
  host: "localhost",
  port: 8080,
  serviceUrl: "https://page-docs.knotree.org",
  publicDomain: "page-docs.knotree.org",
  containerName: "knotree-app-html",
}

describe("HtmlSourceEditor", () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mocks.getHtmlPageIndex.mockResolvedValue({
      indexHtml: "<html><body>hello</body></html>",
    })
    mocks.updateHtmlPage.mockResolvedValue({
      ...pastedPage,
      status: "provisioning",
    })
  })

  it("saves pasted index.html and starts a redeploy", async () => {
    const user = userEvent.setup()
    const onToast = vi.fn()
    const onAppServiceUpdated = vi.fn()
    render(
      <HtmlSourceEditor
        appService={pastedPage}
        workspaceSlug="ws"
        projectSlug="proj"
        onToast={onToast}
        onAppServiceUpdated={onAppServiceUpdated}
      />
    )

    const editor = await screen.findByLabelText("index.html")
    await user.clear(editor)
    await user.type(editor, "<html><body>updated-v2</body></html>")
    await user.click(screen.getByRole("button", { name: "Save and deploy" }))

    await waitFor(() => {
      expect(mocks.updateHtmlPage).toHaveBeenCalledWith("ws", "proj", "html-id", {
        indexHtml: "<html><body>updated-v2</body></html>",
      })
    })
    expect(onToast).toHaveBeenCalledWith("HTML page redeploy started.")
    expect(onAppServiceUpdated).toHaveBeenCalled()
  })

  it("shows GitHub Pages source instead of the paste editor", async () => {
    render(
      <HtmlSourceEditor
        appService={{
          ...pastedPage,
          imageSource: "html_github",
          htmlRepo: "acme/site",
          htmlBranch: "main",
          htmlSha: "deadbeef",
        }}
        workspaceSlug="ws"
        projectSlug="proj"
        onToast={vi.fn()}
      />
    )

    expect(await screen.findByText("acme/site")).toBeInTheDocument()
    expect(screen.getByText("main")).toBeInTheDocument()
    expect(screen.queryByLabelText("index.html")).not.toBeInTheDocument()
  })
})
