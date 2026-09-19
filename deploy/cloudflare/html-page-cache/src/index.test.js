import assert from "node:assert/strict"
import test from "node:test"
import { isCacheableOriginResponse, isHtmlPageHost } from "./index.js"

test("only page- hosts on knotree.org are HTML cache candidates", () => {
  assert.equal(isHtmlPageHost("page-docs.knotree.org"), true)
  assert.equal(isHtmlPageHost("app-docs.knotree.org"), false)
  assert.equal(isHtmlPageHost("page-docs.example.com"), false)
})

test("does not treat error responses as cacheable HTML", () => {
  assert.equal(
    isCacheableOriginResponse(
      new Response("{}", {
        status: 404,
        headers: { "Cache-Control": "max-age=14400" },
      })
    ),
    false
  )
})

test("does not cache partial responses or responses with Vary: *", () => {
  const cacheControl = "public, max-age=60, s-maxage=60"
  assert.equal(
    isCacheableOriginResponse(
      new Response("partial", {
        status: 206,
        headers: { "Cache-Control": cacheControl },
      })
    ),
    false
  )
  assert.equal(
    isCacheableOriginResponse(
      new Response("ok", {
        headers: {
          "Cache-Control": cacheControl,
          Vary: "Accept-Language, *",
        },
      })
    ),
    false
  )
})

test("respects origin Cache-Control for Cloudflare cache eligibility", () => {
  assert.equal(
    isCacheableOriginResponse(
      new Response("ok", {
        headers: {
          "Cache-Control": "public, max-age=60, s-maxage=60",
        },
      })
    ),
    true
  )
  assert.equal(
    isCacheableOriginResponse(
      new Response("ok", {
        headers: { "Cache-Control": "no-store" },
      })
    ),
    false
  )
  assert.equal(
    isCacheableOriginResponse(new Response("missing", { status: 404 })),
    false
  )
})
