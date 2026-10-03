import { afterEach, describe, expect, it, vi } from "vitest"

import { apiRequest, resetCsrfToken } from "@/lib/api"

function json(status: number, body: unknown) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  })
}

describe("apiRequest", () => {
  afterEach(() => {
    vi.unstubAllGlobals()
    resetCsrfToken()
  })

  it("refreshes a stale CSRF token and retries the mutation once", async () => {
    const tokens = ["stale", "fresh"]
    const sent: (string | null)[] = []
    const fetchMock = vi.fn((url: string, init?: RequestInit) => {
      if (url.endsWith("/auth/csrf")) {
        return Promise.resolve(json(200, { csrfToken: tokens.shift() }))
      }
      const token = new Headers(init?.headers).get("X-CSRF-Token")
      sent.push(token)
      return Promise.resolve(
        token === "fresh"
          ? json(200, { ok: true })
          : json(403, {
              error: { code: "CSRF_INVALID", message: "The CSRF token is invalid." },
            })
      )
    })
    vi.stubGlobal("fetch", fetchMock)

    await expect(
      apiRequest("/integrations/knotree-registry/authorize", { method: "POST" })
    ).resolves.toEqual({ ok: true })
    expect(sent).toEqual(["stale", "fresh"])
  })
})
