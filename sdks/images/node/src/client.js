export class KnotreeImagesError extends Error {
  constructor(message, details = {}) {
    super(message)
    this.name = "KnotreeImagesError"
    this.status = details.status ?? 0
    this.code = details.code ?? "IMAGE_REQUEST_FAILED"
    this.fields = details.fields ?? null
  }
}

function normalizeBaseUrl(baseUrl) {
  if (typeof baseUrl !== "string" || baseUrl.trim() === "") {
    throw new KnotreeImagesError("baseUrl is required.", {
      code: "IMAGE_CONFIG",
    })
  }
  return baseUrl.trim().replace(/\/+$/, "").replace(/\/api\/v1$/, "")
}

async function readError(response) {
  let payload = null
  try {
    payload = await response.json()
  } catch {
    payload = null
  }
  const error = payload?.error ?? {}
  return new KnotreeImagesError(
    error.message || `Image request failed (${response.status}).`,
    {
      status: response.status,
      code: error.code || "IMAGE_REQUEST_FAILED",
      fields: error.fields ?? null,
    }
  )
}

export function createKnotreeImages({
  clientId,
  clientSecret,
  baseUrl,
  fetch: fetchImpl,
} = {}) {
  if (!clientId || !clientSecret) {
    throw new KnotreeImagesError("clientId and clientSecret are required.", {
      code: "IMAGE_CONFIG",
    })
  }
  const origin = normalizeBaseUrl(baseUrl)
  const requestFetch = fetchImpl ?? globalThis.fetch.bind(globalThis)

  async function request(path, { method = "GET", query, json, body, headers } = {}) {
    const url = new URL(`${origin}/api/v1${path}`)
    if (query) {
      for (const [key, value] of Object.entries(query)) {
        if (value !== undefined && value !== null && value !== "") {
          url.searchParams.set(key, String(value))
        }
      }
    }
    const initHeaders = {
      "X-Knotree-Client-Id": clientId,
      "X-Knotree-Client-Secret": clientSecret,
      ...headers,
    }
    const init = { method, headers: initHeaders }
    if (json !== undefined) {
      init.headers["Content-Type"] = "application/json"
      init.body = JSON.stringify(json)
    } else if (body !== undefined) {
      init.body = body
    }
    const response = await requestFetch(url, init)
    if (response.status === 204) {
      return null
    }
    if (!response.ok) {
      throw await readError(response)
    }
    const text = await response.text()
    return text ? JSON.parse(text) : null
  }

  return {
    clientId,
    clientSecret,
    baseUrl: origin,
    getStore() {
      return request("/images/store")
    },
    upload({ body, contentType, folder = "", fileName }) {
      if (!fileName) {
        throw new KnotreeImagesError("fileName is required.", {
          code: "IMAGE_CONFIG",
        })
      }
      return request("/images/objects", {
        method: "POST",
        body,
        headers: {
          "Content-Type": contentType || "application/octet-stream",
          "X-Knotree-Folder": folder,
          "X-Knotree-File-Name": fileName,
        },
      })
    },
    list({ folder, recursive = false } = {}) {
      return request("/images/objects", {
        query: {
          folder,
          recursive: recursive ? "true" : "false",
        },
      })
    },
    delete(imageId) {
      return request(`/images/objects/${encodeURIComponent(imageId)}`, {
        method: "DELETE",
      })
    },
    deleteFolder(folder) {
      return request("/images/folders", {
        method: "DELETE",
        query: { folder },
      })
    },
    signUrl(imageId, options = {}) {
      const payload = { visibility: options.visibility ?? "public" }
      if (options.expiresInSeconds != null) {
        payload.expiresInSeconds = options.expiresInSeconds
      }
      if (options.width != null) {
        payload.width = options.width
      }
      if (options.height != null) {
        payload.height = options.height
      }
      if (options.quality != null) {
        payload.quality = options.quality
      }
      return request(`/images/objects/${encodeURIComponent(imageId)}/sign`, {
        method: "POST",
        json: payload,
      })
    },
  }
}
