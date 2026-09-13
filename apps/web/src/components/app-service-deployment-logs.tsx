import type { AppServiceDeployment } from "@/lib/types"

export function AppServiceDeploymentLogs({
  deployment,
  compact = false,
}: {
  deployment: AppServiceDeployment
  compact?: boolean
}) {
  const isActive = deployment.status === "provisioning"
  const statusLabel = isActive
    ? "LIVE"
    : deployment.status === "ready"
      ? "READY"
      : "FAILED"

  return (
    <section
      className={`app-service-deployment-logs${compact ? "compact" : ""}`}
      aria-label="Deployment logs"
    >
      <div className="app-service-deployment-logs-header">
        <div>
          <span
            className={`app-service-deployment-status ${deployment.status}`}
          >
            {statusLabel}
          </span>
          <strong>{deployment.currentStep}</strong>
        </div>
        {isActive && (
          <span className="app-service-deployment-live">Streaming</span>
        )}
      </div>
      <pre className="app-service-deployment-log" role="log" aria-live="polite">
        {deployment.logs.length > 0
          ? deployment.logs.join("\n")
          : "Waiting for deployment logs..."}
      </pre>
      {deployment.errorMessage && (
        <p className="app-service-deployment-error" role="alert">
          {deployment.errorMessage}
        </p>
      )}
    </section>
  )
}
