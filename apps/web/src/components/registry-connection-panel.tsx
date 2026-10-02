import { useEffect, useState, type FormEvent } from "react"
import {
  BoxesIcon,
  RefreshCwIcon,
  RocketIcon,
  ShieldCheckIcon,
  UploadIcon,
} from "lucide-react"
import { cn } from "cn"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { registryAuthorizationTarget } from "@/lib/registry-consent"
import {
  attachAppServiceRegistryConnection,
  createKnotreeRegistryConnection,
  deleteKnotreeRegistryConnection,
  listAppServices,
  listKnotreeRegistryConnections,
  listRegistryDeploys,
  startKnotreeRegistryConsent,
  updateAppServiceAutoDeploy,
  updateKnotreeRegistryConnection,
} from "@/lib/resources"
import type {
  AppService,
  KnotreeRegistryConnectionList,
  RegistryDeployHistory,
  RegistryDeployJob,
} from "@/lib/types"

const REGISTRY_HOST = "registry.knotree.com"

type Tone = "ok" | "warn" | "danger" | "muted"

function parseRegistryImage(image: string) {
  const withoutHost = image.startsWith(`${REGISTRY_HOST}/`)
    ? image.slice(REGISTRY_HOST.length + 1)
    : image
  const separator = withoutHost.lastIndexOf(":")
  if (separator <= 0) return { repository: withoutHost, tag: "latest" }
  return {
    repository: withoutHost.slice(0, separator),
    tag: withoutHost.slice(separator + 1),
  }
}

function shortDigest(digest: string) {
  return /^sha256:[0-9a-f]{64}$/.test(digest) ? digest.slice(0, 19) : digest
}

function relativeTime(value: string, now = Date.now()) {
  const seconds = Math.round((new Date(value).getTime() - now) / 1000)
  const format = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" })
  const abs = Math.abs(seconds)
  if (abs < 60) return format.format(seconds, "second")
  if (abs < 3600) return format.format(Math.round(seconds / 60), "minute")
  if (abs < 86_400) return format.format(Math.round(seconds / 3600), "hour")
  return format.format(Math.round(seconds / 86_400), "day")
}

function jobLabel(job: RegistryDeployJob): { label: string; tone: Tone } {
  switch (job.status) {
    case "pending":
      return { label: "Queued", tone: "muted" }
    case "running":
      return { label: "Deploying", tone: "warn" }
    case "failed":
      return { label: "Failed", tone: "danger" }
    default:
      // A succeeded job without a deployment was a no-op: the digest was
      // already running, or a newer push for the same tag replaced it.
      return job.deploymentId
        ? { label: "Deployed", tone: "ok" }
        : { label: "Skipped", tone: "muted" }
  }
}

function connectionState(
  connected: boolean,
  enabled: boolean,
  autoDeployReady: boolean | null,
  tag: string
): { label: string; tone: Tone; text: string } {
  if (!connected) {
    return {
      label: "Disconnected",
      tone: "danger",
      text: "Cloud cannot pull new images for this service. The current version keeps running. Reconnect to resume pulls and automatic deploys.",
    }
  }
  if (autoDeployReady === false) {
    return {
      label: "Unavailable",
      tone: "muted",
      text: "Automatic Registry deploys are not configured on this Cloud installation yet. Manual deploys from the connected repository still work.",
    }
  }
  if (enabled) {
    return {
      label: "Live",
      tone: "ok",
      text: `Every push to the ${tag} tag deploys its exact image digest automatically.`,
    }
  }
  return {
    label: "Paused",
    tone: "warn",
    text: `Pushes to the ${tag} tag are received but not deployed. Turn on auto deploy to resume.`,
  }
}

export function RegistryConnectionPanel({
  appService,
  workspaceId,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  workspaceId: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const { repository, tag } = parseRegistryImage(appService.image)
  const connected = Boolean(appService.registryConnectionId)
  const [enabled, setEnabled] = useState(appService.autoDeployEnabled ?? false)
  const [history, setHistory] = useState<RegistryDeployHistory | null>(null)
  const [historyError, setHistoryError] = useState<string | null>(null)
  const [historyVersion, setHistoryVersion] = useState(0)
  const [isSaving, setIsSaving] = useState(false)
  const [isDisconnecting, setIsDisconnecting] = useState(false)
  const [isRotatingToken, setIsRotatingToken] = useState(false)
  const [replacementToken, setReplacementToken] = useState("")
  const [error, setError] = useState<string | null>(null)
  const isBusy = isSaving || appService.status === "provisioning"

  useEffect(() => {
    let cancelled = false
    listRegistryDeploys(workspaceId, projectSlug, appService.id)
      .then((result) => {
        if (cancelled) return
        setHistory(result)
        setHistoryError(null)
      })
      .catch((caught: unknown) => {
        if (cancelled) return
        setHistoryError(
          caught instanceof ApiError
            ? caught.message
            : "Recent Registry pushes could not be loaded."
        )
      })
    return () => {
      cancelled = true
    }
  }, [
    workspaceId,
    projectSlug,
    appService.id,
    appService.deployedImageDigest,
    appService.autoDeployCheckedAt,
    historyVersion,
  ])

  const autoDeployReady = history?.autoDeployReady ?? null
  const state = connectionState(connected, enabled, autoDeployReady, tag)

  async function handleToggle(nextEnabled: boolean) {
    setEnabled(nextEnabled)
    setIsSaving(true)
    setError(null)
    try {
      const resource = await updateAppServiceAutoDeploy(
        workspaceId,
        projectSlug,
        appService.id,
        { enabled: nextEnabled }
      )
      setEnabled(resource.autoDeployEnabled ?? nextEnabled)
      onAppServiceUpdated?.(resource)
      onToast(
        nextEnabled
          ? "Automatic Knotree Registry deploys enabled."
          : "Automatic Knotree Registry deploys disabled."
      )
    } catch (caught) {
      setEnabled(appService.autoDeployEnabled ?? false)
      setError(
        caught instanceof ApiError
          ? caught.message
          : "Automatic image deploys could not be updated."
      )
    } finally {
      setIsSaving(false)
    }
  }

  async function disconnect() {
    const connectionId = appService.registryConnectionId
    if (!connectionId) return
    setIsDisconnecting(true)
    setError(null)
    try {
      await deleteKnotreeRegistryConnection(
        workspaceId,
        projectSlug,
        connectionId
      )
      const services = await listAppServices(workspaceId, projectSlug)
      const updated = services.find((service) => service.id === appService.id)
      if (updated) onAppServiceUpdated?.(updated)
      onToast(
        "Cloud connection removed. Revoke the PAT in Knotree Registry too if you no longer need it. The running service stays up; future Cloud pulls and auto-deploys are stopped."
      )
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : "The Knotree Registry connection could not be disconnected."
      )
    } finally {
      setIsDisconnecting(false)
    }
  }

  async function rotateToken(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const connectionId = appService.registryConnectionId
    if (!connectionId || !replacementToken.trim()) return
    setIsRotatingToken(true)
    setError(null)
    try {
      await updateKnotreeRegistryConnection(
        workspaceId,
        projectSlug,
        connectionId,
        replacementToken.trim()
      )
      setReplacementToken("")
      onToast("Knotree Registry token updated for this project connection.")
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : "The Knotree Registry token could not be updated."
      )
    } finally {
      setIsRotatingToken(false)
    }
  }

  return (
    <section
      className="registry-panel"
      aria-label="Knotree Registry connection"
    >
      <header className="registry-panel-head">
        <span className="registry-panel-icon" aria-hidden="true">
          <BoxesIcon />
        </span>
        <div className="registry-panel-title">
          <strong>Knotree Registry</strong>
          <code>
            {REGISTRY_HOST}/{repository}:{tag}
          </code>
        </div>
        <span className={cn("registry-panel-pill", state.tone)}>
          {state.label}
        </span>
      </header>
      <p className="registry-panel-summary">{state.text}</p>

      <ol className="registry-panel-flow" aria-label="How auto deploy works">
        <li>
          <UploadIcon aria-hidden="true" />
          <div>
            <strong>Push</strong>
            <span>
              You push a new image to the <code>{tag}</code> tag.
            </span>
          </div>
        </li>
        <li>
          <ShieldCheckIcon aria-hidden="true" />
          <div>
            <strong>Signed event</strong>
            <span>Registry notifies Cloud with an HMAC-signed tag update.</span>
          </div>
        </li>
        <li>
          <RocketIcon aria-hidden="true" />
          <div>
            <strong>Deploy</strong>
            <span>Cloud rolls out the exact image digest that was pushed.</span>
          </div>
        </li>
      </ol>

      {connected ? (
        <div className="registry-panel-toggle">
          <div>
            <strong>Auto deploy</strong>
            <span>Deploy each new digest pushed to {tag}.</span>
          </div>
          <label>
            <input
              type="checkbox"
              aria-label="Auto deploy new Knotree Registry images"
              checked={enabled}
              disabled={isBusy || autoDeployReady === false}
              onChange={(event) => void handleToggle(event.target.checked)}
            />
            <span>
              {isSaving ? "Saving…" : enabled ? "Enabled" : "Disabled"}
            </span>
          </label>
        </div>
      ) : null}

      <dl className="registry-panel-facts">
        <div>
          <dt>Repository</dt>
          <dd>
            <code>{repository}</code>
          </dd>
        </div>
        <div>
          <dt>Watched tag</dt>
          <dd>
            <code>{tag}</code>
          </dd>
        </div>
        <div>
          <dt>Deployed digest</dt>
          <dd>
            <code title={appService.deployedImageDigest ?? undefined}>
              {appService.deployedImageDigest
                ? shortDigest(appService.deployedImageDigest)
                : "Not recorded yet"}
            </code>
          </dd>
        </div>
        <div>
          <dt>Last Registry event</dt>
          <dd>
            {appService.autoDeployCheckedAt
              ? new Date(appService.autoDeployCheckedAt).toLocaleString()
              : "None yet"}
          </dd>
        </div>
      </dl>

      <div className="registry-panel-history">
        <div className="registry-panel-history-head">
          <h4>Recent pushes</h4>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Refresh recent pushes"
            onClick={() => setHistoryVersion((value) => value + 1)}
          >
            <RefreshCwIcon />
          </Button>
        </div>
        {historyError ? (
          <p className="registry-panel-error" role="alert">
            {historyError}
          </p>
        ) : !history ? (
          <p className="registry-panel-muted" role="status">
            <Spinner /> Loading recent pushes…
          </p>
        ) : history.jobs.length === 0 ? (
          <p className="registry-panel-empty">
            No pushes received yet. Push a new image to <code>{tag}</code> and
            it appears here.
          </p>
        ) : (
          <ul className="registry-panel-jobs">
            {history.jobs.map((job) => {
              const { label, tone } = jobLabel(job)
              return (
                <li key={job.id}>
                  <span className={cn("registry-panel-chip", tone)}>
                    {label}
                  </span>
                  <code title={job.imageDigest}>
                    {shortDigest(job.imageDigest)}
                  </code>
                  <time dateTime={job.receivedAt} title={job.receivedAt}>
                    {relativeTime(job.receivedAt)}
                  </time>
                  {job.status === "failed" && job.lastError ? (
                    <p>{job.lastError}</p>
                  ) : null}
                </li>
              )
            })}
          </ul>
        )}
      </div>

      {connected ? (
        <div className="registry-panel-actions">
          <form onSubmit={rotateToken}>
            <label htmlFor="replacementRegistryToken">Replace pull token</label>
            <div>
              <Input
                id="replacementRegistryToken"
                type="password"
                autoComplete="new-password"
                value={replacementToken}
                onChange={(event) => setReplacementToken(event.target.value)}
                placeholder="Paste a new pull-only token"
              />
              <Button
                type="submit"
                variant="outline"
                size="sm"
                disabled={
                  isRotatingToken ||
                  isBusy ||
                  replacementToken.trim().length === 0
                }
              >
                {isRotatingToken ? <Spinner data-icon="inline-start" /> : null}
                {isRotatingToken ? "Updating…" : "Update token"}
              </Button>
            </div>
            <small>
              The replacement is verified for this repository and remains
              encrypted. Disconnecting removes Cloud&apos;s saved connection;
              revoke the PAT in Knotree Registry separately if needed.
            </small>
          </form>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isDisconnecting || isRotatingToken || isBusy}
            onClick={() => void disconnect()}
          >
            {isDisconnecting ? <Spinner data-icon="inline-start" /> : null}
            {isDisconnecting ? "Disconnecting…" : "Disconnect Knotree Registry"}
          </Button>
        </div>
      ) : (
        <RegistryReconnect
          appService={appService}
          repository={repository}
          workspaceId={workspaceId}
          projectSlug={projectSlug}
          onToast={onToast}
          onAppServiceUpdated={onAppServiceUpdated}
        />
      )}

      {appService.autoDeployError && connected ? (
        <p className="registry-panel-error" role="alert">
          {appService.autoDeployError}
        </p>
      ) : null}
      {error ? (
        <p className="registry-panel-error" role="alert">
          {error}
        </p>
      ) : null}
    </section>
  )
}

function RegistryReconnect({
  appService,
  repository,
  workspaceId,
  projectSlug,
  onToast,
  onAppServiceUpdated,
}: {
  appService: AppService
  repository: string
  workspaceId: string
  projectSlug: string
  onToast: (message: string) => void
  onAppServiceUpdated?: (resource: AppService) => void
}) {
  const [connections, setConnections] =
    useState<KnotreeRegistryConnectionList | null>(null)
  const [selected, setSelected] = useState("")
  const [username, setUsername] = useState("")
  const [token, setToken] = useState("")
  const [busy, setBusy] = useState<"attach" | "create" | "consent" | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    listKnotreeRegistryConnections(workspaceId, projectSlug)
      .then((result) => {
        if (!cancelled) setConnections(result)
      })
      .catch(() => {
        if (!cancelled) {
          setConnections({ connections: [], autoDeployReady: false })
        }
      })
    return () => {
      cancelled = true
    }
  }, [workspaceId, projectSlug])

  const matching =
    connections?.connections.filter(
      (connection) => connection.repository === repository
    ) ?? []
  const selectedId = selected || matching[0]?.id || ""

  async function attach(connectionId: string) {
    const resource = await attachAppServiceRegistryConnection(
      workspaceId,
      projectSlug,
      appService.id,
      connectionId
    )
    onAppServiceUpdated?.(resource)
    onToast(
      "Knotree Registry reconnected. Turn on auto deploy to deploy new pushes automatically."
    )
  }

  async function attachSaved() {
    if (!selectedId) return
    setBusy("attach")
    setError(null)
    try {
      await attach(selectedId)
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : "The Registry connection could not be attached."
      )
    } finally {
      setBusy(null)
    }
  }

  async function connectWithToken(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!username.trim() || !token.trim()) return
    setBusy("create")
    setError(null)
    try {
      const connection = await createKnotreeRegistryConnection(
        workspaceId,
        projectSlug,
        { username: username.trim(), token: token.trim(), repository }
      )
      setToken("")
      await attach(connection.id)
    } catch (caught) {
      setError(
        caught instanceof ApiError
          ? caught.message
          : "Knotree Registry could not verify this token."
      )
    } finally {
      setBusy(null)
    }
  }

  async function authorizeOnRegistry() {
    setBusy("consent")
    setError(null)
    try {
      const result = await startKnotreeRegistryConsent(
        workspaceId,
        projectSlug,
        repository
      )
      window.location.assign(
        registryAuthorizationTarget(result.authorizationUrl)
      )
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Registry authorization failed."
      )
      setBusy(null)
    }
  }

  return (
    <div className="registry-panel-reconnect">
      <h4>Reconnect</h4>
      <p className="registry-panel-muted">
        Cloud needs pull access to <code>repository:{repository}:pull</code>.
        The service is not recreated and keeps its URL, variables and database.
      </p>
      {connections === null ? (
        <p className="registry-panel-muted" role="status">
          <Spinner /> Loading saved connections…
        </p>
      ) : null}
      {matching.length ? (
        <div className="registry-panel-reconnect-row">
          <label htmlFor="registryReconnectChoice">Saved connection</label>
          <div>
            <select
              id="registryReconnectChoice"
              value={selectedId}
              onChange={(event) => setSelected(event.target.value)}
            >
              {matching.map((connection) => (
                <option key={connection.id} value={connection.id}>
                  {connection.repository} · @{connection.username}
                </option>
              ))}
            </select>
            <Button
              type="button"
              size="sm"
              disabled={busy !== null}
              onClick={() => void attachSaved()}
            >
              {busy === "attach" ? <Spinner data-icon="inline-start" /> : null}
              Reconnect
            </Button>
          </div>
        </div>
      ) : null}
      {connections?.consentReady ? (
        <div className="registry-panel-reconnect-row">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={busy !== null}
            onClick={() => void authorizeOnRegistry()}
          >
            {busy === "consent" ? <Spinner data-icon="inline-start" /> : null}
            Authorize on Registry
          </Button>
          <small>
            Approve pull access on Registry, then come back and pick the saved
            connection above.
          </small>
        </div>
      ) : null}
      <form
        className="registry-panel-reconnect-form"
        onSubmit={connectWithToken}
      >
        <div>
          <label htmlFor="registryReconnectUsername">Registry username</label>
          <Input
            id="registryReconnectUsername"
            autoComplete="username"
            value={username}
            onChange={(event) => setUsername(event.target.value)}
          />
        </div>
        <div>
          <label htmlFor="registryReconnectToken">Pull-only token</label>
          <Input
            id="registryReconnectToken"
            type="password"
            autoComplete="new-password"
            value={token}
            onChange={(event) => setToken(event.target.value)}
          />
        </div>
        <Button
          type="submit"
          variant={matching.length ? "outline" : "default"}
          size="sm"
          disabled={busy !== null || !username.trim() || !token.trim()}
        >
          {busy === "create" ? <Spinner data-icon="inline-start" /> : null}
          Connect with token
        </Button>
      </form>
      {error ? (
        <p className="registry-panel-error" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  )
}
