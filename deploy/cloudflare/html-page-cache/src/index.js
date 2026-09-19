/**
 * Cache GET requests for page-* HTML hosts at Cloudflare so static pages
 * do not hit origin on every request. Other app-service hosts pass through.
 */

export function isHtmlPageHost(hostname) {
  return hostname.startsWith("page-") && hostname.endsWith(".knotree.org")
}

export function isCacheableOriginResponse(response) {
  if (!response.ok) {
    return false
  }
  const cacheControl = response.headers.get("Cache-Control") || ""
  if (/no-store|private|no-cache/i.test(cacheControl)) {
    return false
  }
  return /s-maxage=\d+|max-age=\d+/i.test(cacheControl)
}

export default {
  async fetch(request, _env, ctx) {
    const url = new URL(request.url)
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
