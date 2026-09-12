import { describe, expect, it } from "vitest"

import { slugifyWorkspaceName } from "@/lib/slug"

describe("slugifyWorkspaceName", () => {
  it("turns names into stable kebab-case slugs", () => {
    expect(slugifyWorkspaceName("  Cà phê Studio  ")).toBe("ca-phe-studio")
  })

  it("limits the slug to the API length", () => {
    expect(slugifyWorkspaceName("a".repeat(60))).toHaveLength(48)
  })
})
