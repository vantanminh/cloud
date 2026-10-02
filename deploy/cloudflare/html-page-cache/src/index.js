/**
 * Cache GET requests for page-* HTML hosts and for img.knotree.org.
 * Public image URLs keep the origin's one-year cache. Private image URLs
 * are cached for 60 seconds. Other app-service hosts pass through.
 */

export const PUBLIC_IMAGE_CACHE_CONTROL = "public, max-age=31536000, immutable"
export const PRIVATE_IMAGE_CACHE_CONTROL = "public, max-age=60"
export const PUBLIC_IMAGE_CDN_CACHE_CONTROL = "public, max-age=31536000"
export const PRIVATE_IMAGE_CDN_CACHE_CONTROL = "public, max-age=60"

export function isHtmlPageHost(hostname) {
  return hostname.startsWith("page-") && hostname.endsWith(".knotree.org")
}

export function isImageHost(hostname) {
  return hostname === "img.knotree.org"
}

export function isPrivateImageUrl(url) {
  return url.searchParams.has("exp") || url.searchParams.has("kid")
}

export function imageCacheControl(url, originCacheControl = "") {
  if (isPrivateImageUrl(url)) {
    return PRIVATE_IMAGE_CACHE_CONTROL
  }
  if (
    /max-age=\d+/i.test(originCacheControl) &&
    !/no-store|private|no-cache/i.test(originCacheControl)
  ) {
    return originCacheControl
  }
  return PUBLIC_IMAGE_CACHE_CONTROL
}

export function shouldCacheImageResponse(response, url) {
  if (!response.ok || response.status === 206) {
    return false
  }
  if (isPrivateImageUrl(url)) {
    return true
  }
  const cacheControl = response.headers.get("Cache-Control") || ""
  return !/no-store|private|no-cache/i.test(cacheControl)
}

export function cachedImageHeaders(url, originHeaders) {
  const headers = new Headers(originHeaders)
  const originCache = headers.get("Cache-Control") || ""
  headers.set("Cache-Control", imageCacheControl(url, originCache))
  headers.set(
    "CDN-Cache-Control",
    isPrivateImageUrl(url)
      ? PRIVATE_IMAGE_CDN_CACHE_CONTROL
      : headers.get("CDN-Cache-Control") || PUBLIC_IMAGE_CDN_CACHE_CONTROL
  )
  headers.set("Access-Control-Allow-Origin", "*")
  headers.delete("Vary")
  return headers
}

export function isCacheableOriginResponse(response) {
  if (!response.ok || response.status === 206) {
    return false
  }
  const cacheControl = response.headers.get("Cache-Control") || ""
  if (/no-store|private|no-cache/i.test(cacheControl)) {
    return false
  }
  const vary = response.headers.get("Vary") || ""
  if (vary.split(",").some((value) => value.trim() === "*")) {
    return false
  }
  return /s-maxage=\d+|max-age=\d+/i.test(cacheControl)
}

async function serveCachedImage(request, url, ctx) {
  if (request.method !== "GET") {
    return fetch(request)
  }

  const cache = caches.default
  const cacheKey = new Request(url.toString(), { method: "GET" })
  const cached = await cache.match(cacheKey)
  if (cached && cached.ok) {
    const hit = new Response(cached.body, cached)
    hit.headers.set("Access-Control-Allow-Origin", "*")
    hit.headers.set("CF-Cache-Status", "HIT")
    hit.headers.set("X-Knotree-Image-Cache", "HIT")
    return hit
  }

  const origin = await fetch(request)
  if (!shouldCacheImageResponse(origin, url)) {
    const bypass = new Response(origin.body, origin)
    bypass.headers.set("Cache-Control", "no-store")
    bypass.headers.set("X-Knotree-Image-Cache", "BYPASS")
    return bypass
  }

  const stored = new Response(origin.body, {
    status: origin.status,
    headers: cachedImageHeaders(url, origin.headers),
  })
  ctx.waitUntil(cache.put(cacheKey, stored.clone()))
  const miss = new Response(stored.body, stored)
  miss.headers.set("X-Knotree-Image-Cache", "MISS")
  return miss
}

export default {
  async fetch(request, _env, ctx) {
    const url = new URL(request.url)
    if (isImageHost(url.hostname)) {
      return serveCachedImage(request, url, ctx)
    }
    if (
      !isHtmlPageHost(url.hostname) ||
      request.method !== "GET"
    ) {
      return fetch(request)
    }

    const cache = caches.default
    const cacheKey = new Request(url.toString(), {
      method: "GET",
      headers: request.headers,
    })
    const cached = await cache.match(cacheKey)
    if (cached && cached.ok) {
      const hit = new Response(cached.body, cached)
      hit.headers.set("CF-Cache-Status", "HIT")
      hit.headers.set("X-Knotree-Html-Cache", "HIT")
      return hit
    }

    const origin = await fetch(request)
    if (!origin.ok) {
      const bypass = new Response(origin.body, origin)
      bypass.headers.set("Cache-Control", "no-store")
      bypass.headers.set("X-Knotree-Html-Cache", "BYPASS")
      return bypass
    }
    if (isCacheableOriginResponse(origin)) {
      const toStore = origin.clone()
      ctx.waitUntil(cache.put(cacheKey, toStore))
    }
    const miss = new Response(origin.body, origin)
    miss.headers.set("X-Knotree-Html-Cache", "MISS")
    return miss
  },
}
