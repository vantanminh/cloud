import { describe, expect, it } from "vitest"

import { slugifyProjectName } from "@/lib/slug"

describe("slugifyProjectName", () => {
  it("turns names into stable kebab-case slugs", () => {
    expect(slugifyProjectName("  Cà phê Studio  ")).toBe("ca-phe-studio")
  })

  it("limits the slug to the API length", () => {
    expect(slugifyProjectName("a".repeat(60))).toHaveLength(48)
  })
})
