import { useCallback, useEffect, useState } from "react"
import { RefreshCwIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { ApiError } from "@/lib/api"
import { getAppServiceLogs } from "@/lib/resources"
import type { AppService, AppServiceLogs } from "@/lib/types"

import "./app-service-runtime-logs.css"

const LOG_REFRESH_MS = 3_000

export function AppServiceRuntimeLogs({
  appService,
  workspaceId,
  projectSlug,
  compact = false,
}: {
  appService: AppService
  workspaceId: string
  projectSlug: string
  compact?: boolean
}) {
  const [logs, setLogs] = useState<AppServiceLogs | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [isRefreshing, setIsRefreshing] = useState(false)
  const [live, setLive] = useState(true)

  const refresh = useCallback(async () => {
    setIsRefreshing(true)
    try {
      const nextLogs = await getAppServiceLogs(
        workspaceId,
        projectSlug,
        appService.id
      )
      setLogs(nextLogs)
      setError(null)
    } catch (requestError: unknown) {
      setError(
        requestError instanceof ApiError
          ? requestError.message
          : "The app service logs could not be loaded."
      )
    } finally {
      setIsLoading(false)
      setIsRefreshing(false)
    }
  }, [appService.id, projectSlug, workspaceId])

  useEffect(() => {
    const timeoutId = window.setTimeout(() => {
      void refresh()
    })
    return () => window.clearTimeout(timeoutId)
  }, [refresh])

  useEffect(() => {
    if (!live) {
      return undefined
    }

    const intervalId = window.setInterval(() => {
      void refresh()
    }, LOG_REFRESH_MS)
    return () => window.clearInterval(intervalId)
  }, [live, refresh])

  const statusLabel = isLoading
    ? "LOADING"
    : logs?.running
      ? "RUNNING"
      : logs?.status === "ready"
        ? "STOPPED"
        : "WAITING"
  const lines = logs?.lines ?? []

  return (
    <section
      className={`app-service-runtime-logs${compact ? " compact" : ""}`}
      aria-label="App service runtime logs"
    >
      <div className="app-service-runtime-logs-header">
        <div>
          <span
            className={`app-service-runtime-status ${statusLabel.toLowerCase()}`}
          >
            {statusLabel}
          </span>
          <strong>{logs?.containerName ?? appService.name}</strong>
        </div>
        <div className="app-service-runtime-logs-controls">
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Refresh app service logs"
            title="Refresh app service logs"
            onClick={() => void refresh()}
            disabled={isRefreshing}
          >
            <RefreshCwIcon
              className={isRefreshing ? "is-spinning" : undefined}
            />
          </Button>
          <Button
            type="button"
            variant={live ? "secondary" : "ghost"}
            size="sm"
            aria-pressed={live}
            onClick={() => setLive((current) => !current)}
          >
            {live ? "Live" : "Paused"}
          </Button>
        </div>
      </div>
      {error && (
        <p className="app-service-runtime-logs-error" role="alert">
          {error}
        </p>
      )}
      {isLoading ? (
        <div className="app-service-runtime-logs-state" role="status">
          Loading runtime logs...
        </div>
      ) : lines.length > 0 ? (
        <pre className="app-service-runtime-log" role="log" aria-live="polite">
          {lines.join("\n")}
        </pre>
      ) : (
        <div className="app-service-runtime-logs-state">
          <strong>{logs?.message ?? "No application output yet."}</strong>
          <span>
            {live
              ? "This view will refresh while the service is running."
              : "Resume live updates to keep watching this service."}
          </span>
        </div>
      )}
    </section>
  )
}
