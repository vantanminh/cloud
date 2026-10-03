import type { ApiErrorPayload } from "@/lib/types"

export const API_BASE_URL = (
  import.meta.env.VITE_API_BASE_URL ??
  (import.meta.env.PROD
    ? `${window.location.origin}/api/v1`
    : "http://localhost:8080/api/v1")
).replace(/\/$/, "")

export function apiUrl(path: string) {
  return `${API_BASE_URL}${path}`
}

let csrfToken: string | null = null

export class ApiError extends Error {
  readonly status: number
  readonly code: string
  readonly fields: Record<string, string>

  constructor(
    status: number,
    code: string,
    message: string,
    fields: Record<string, string> = {}
  ) {
    super(message)
    this.name = "ApiError"
    this.status = status
    this.code = code
    this.fields = fields
  }
}

type ApiRequestOptions = Omit<RequestInit, "body"> & {
  body?: unknown
}

export function resetCsrfToken() {
  csrfToken = null
}

export async function getCsrfToken(): Promise<string> {
  if (csrfToken) {
    return csrfToken
  }

  const response = await fetch(apiUrl("/auth/csrf"), {
    credentials: "include",
    headers: { Accept: "application/json" },
  })
  if (!response.ok) {
    throw await toApiError(response)
  }

  const payload = (await response.json()) as { csrfToken: string }
  csrfToken = payload.csrfToken
  return csrfToken
}

export async function apiBinaryRequest<T>(
  path: string,
  body: Blob,
  headersInit: Record<string, string>
): Promise<T> {
  const headers = new Headers(headersInit)
  headers.set("X-CSRF-Token", await getCsrfToken())
  const response = await fetch(apiUrl(path), {
    method: "POST",
    credentials: "include",
    headers,
    body,
  })
  if (!response.ok) {
    throw await toApiError(response)
  }
  return (await response.json()) as T
}

export async function apiRequest<T>(
  path: string,
  options: ApiRequestOptions = {}
): Promise<T> {
  try {
    return await sendApiRequest<T>(path, options)
  } catch (error) {
    // The CSRF cookie is per browser; if it changed since this tab cached its
    // token (sign-in, logout, another tab), fetch a fresh one and retry once.
    if (
      error instanceof ApiError &&
      error.status === 403 &&
      (error.code === "CSRF_INVALID" || error.code === "CSRF_REQUIRED")
    ) {
      resetCsrfToken()
      return sendApiRequest<T>(path, options)
    }
    throw error
  }
}

async function sendApiRequest<T>(
  path: string,
  options: ApiRequestOptions
): Promise<T> {
  const method = (options.method ?? "GET").toUpperCase()
  const headers = new Headers(options.headers)
  headers.set("Accept", "application/json")

  let body: BodyInit | undefined
  if (options.body !== undefined) {
    headers.set("Content-Type", "application/json")
    body = JSON.stringify(options.body)
  }

  if (method !== "GET" && method !== "HEAD") {
    headers.set("X-CSRF-Token", await getCsrfToken())
  }

  const response = await fetch(apiUrl(path), {
    ...options,
    body,
    credentials: "include",
    headers,
  })

  if (!response.ok) {
    throw await toApiError(response)
  }
  if (response.status === 204) {
    return undefined as T
  }

  return (await response.json()) as T
}

async function toApiError(response: Response): Promise<ApiError> {
  let payload: ApiErrorPayload = {}
  try {
    payload = (await response.json()) as ApiErrorPayload
  } catch {
    // Keep the transport error useful even when the server did not return JSON.
  }

  return new ApiError(
    response.status,
    payload.error?.code ?? "REQUEST_FAILED",
    payload.error?.message ?? "Something went wrong. Please try again.",
    payload.error?.fields ?? {}
  )
}
