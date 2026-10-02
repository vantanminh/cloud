import assert from "node:assert/strict"
import test from "node:test"
import {
  PRIVATE_IMAGE_CACHE_CONTROL,
  PUBLIC_IMAGE_CACHE_CONTROL,
  cachedImageHeaders,
  imageCacheControl,
  isCacheableOriginResponse,
  isHtmlPageHost,
  isImageHost,
  isPrivateImageUrl,
  shouldCacheImageResponse,
} from "./index.js"

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

test("caches public image hosts for one year and private URLs for 60 seconds", () => {
  const publicUrl = new URL(
    "https://img.knotree.org/images/v1/store/image?mode=none&sig=abc"
  )
  const privateUrl = new URL(
    "https://img.knotree.org/images/v1/store/image?mode=none&exp=1&kid=kimg_a&sig=abc"
  )
  assert.equal(isImageHost("img.knotree.org"), true)
  assert.equal(isImageHost("page-docs.knotree.org"), false)
  assert.equal(isPrivateImageUrl(publicUrl), false)
  assert.equal(isPrivateImageUrl(privateUrl), true)
  assert.equal(
    imageCacheControl(publicUrl, "public, max-age=31536000, immutable"),
    "public, max-age=31536000, immutable"
  )
  assert.equal(imageCacheControl(publicUrl, ""), PUBLIC_IMAGE_CACHE_CONTROL)
  assert.equal(imageCacheControl(privateUrl, "no-store"), PRIVATE_IMAGE_CACHE_CONTROL)

  const headers = cachedImageHeaders(privateUrl, {
    "Cache-Control": "public, max-age=31536000, immutable",
    "CDN-Cache-Control": "public, max-age=31536000",
    Vary: "Origin",
    "Access-Control-Allow-Origin": "https://app.example",
  })
  assert.equal(headers.get("Cache-Control"), PRIVATE_IMAGE_CACHE_CONTROL)
  assert.equal(headers.get("CDN-Cache-Control"), "public, max-age=60")
  assert.equal(headers.get("Access-Control-Allow-Origin"), "*")
  assert.equal(headers.get("Vary"), null)
  assert.equal(
    shouldCacheImageResponse(
      new Response("ok", { headers: { "Cache-Control": "no-store" } }),
      privateUrl
    ),
    true
  )
  assert.equal(
    shouldCacheImageResponse(new Response("missing", { status: 404 }), publicUrl),
    false
  )
  assert.equal(
    shouldCacheImageResponse(
      new Response("ok", { headers: { "Cache-Control": "no-store" } }),
      publicUrl
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
