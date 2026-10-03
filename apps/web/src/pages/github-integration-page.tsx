import { useEffect, useState } from "react"
import {
  CheckCircle2Icon,
  FolderKanbanIcon,
  GitBranchIcon,
  PlugIcon,
  RefreshCwIcon,
} from "lucide-react"
import { Link, useNavigate, useSearchParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import {
  AppShell,
  BreadcrumbSeparator,
  SidebarNavItem,
} from "@/components/app-shell"
import { BrandMark } from "@/components/brand-mark"
import { LoadingScreen } from "@/components/loading-screen"
import { KnotreeRegistryAccountCard } from "@/components/knotree-registry-account-card"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import {
  disconnectGithub,
  getGithubAuthorizationUrl,
  getGithubConnectionStatus,
} from "@/lib/resources"
import type { GithubConnectionStatus } from "@/lib/types"

export function GitHubIntegrationPage() {
  const { session, status } = useAuth()
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const [connection, setConnection] = useState<GithubConnectionStatus | null>(
    null
  )
  const [isLoading, setIsLoading] = useState(true)
  const [isConnecting, setIsConnecting] = useState(false)
  const [isDisconnecting, setIsDisconnecting] = useState(false)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    void getGithubConnectionStatus()
      .then((status) => {
        if (active) {
          setConnection(status)
          setLoadError(null)
        }
      })
      .catch((error: unknown) => {
        if (active) {
          setLoadError(
            error instanceof ApiError
              ? error.message
              : "The API is currently unavailable. Please try again."
          )
        }
      })
      .finally(() => {
        if (active) {
          setIsLoading(false)
        }
      })

    return () => {
      active = false
    }
  }, [])

  const callbackStatus = searchParams.get("github")
  const callbackMessage =
    callbackStatus === "connected"
      ? "GitHub is connected. Private ghcr.io images can now be deployed from your projects."
      : callbackStatus === "error"
        ? "GitHub could not be connected. Check the authorization and try again."
        : null
  const workspaceHref = session?.workspace
    ? `/workspace/${session.workspace.id}`
    : "/new/workspace"

  async function connectGithub() {
    setActionError(null)
    setIsConnecting(true)
    try {
      const { authorizationUrl } = await getGithubAuthorizationUrl(
        "/settings/integrations"
      )
      window.location.assign(authorizationUrl)
    } catch (error) {
      setActionError(
        error instanceof ApiError
          ? error.message
          : "GitHub login is currently unavailable. Please try again."
      )
      setIsConnecting(false)
    }
  }

  async function disconnectGithubAccount() {
    setActionError(null)
    setIsDisconnecting(true)
    try {
      await disconnectGithub()
      setConnection({ connected: false, login: null })
      setNotice("GitHub has been disconnected from your Knotree account.")
    } catch (error) {
      setActionError(
        error instanceof ApiError
          ? error.message
          : "GitHub could not be disconnected. Please try again."
      )
    } finally {
      setIsDisconnecting(false)
    }
  }

  const content = (
    <div className="app-content">
      <div className="app-page app-page-narrow">
        <div className="page-header">
          <div>
            <span className="page-eyebrow">Account settings</span>
            <h1>Integrations</h1>
            <p>
              Connect GitHub and Knotree Registry once and use their image
              access across every project you deploy on Knotree.
            </p>
          </div>
        </div>

        <div className="settings-stack">
          {callbackMessage && (
            <Alert
              variant={callbackStatus === "error" ? "destructive" : "default"}
            >
              <CheckCircle2Icon aria-hidden="true" />
              <AlertDescription>{callbackMessage}</AlertDescription>
            </Alert>
          )}
          {notice && (
            <Alert>
              <CheckCircle2Icon aria-hidden="true" />
              <AlertDescription>{notice}</AlertDescription>
            </Alert>
          )}
          {loadError && (
            <Alert variant="destructive">
              <AlertDescription>{loadError}</AlertDescription>
            </Alert>
          )}
          {actionError && (
            <Alert variant="destructive">
              <AlertDescription>{actionError}</AlertDescription>
            </Alert>
          )}

          <section className="settings-card" aria-labelledby="github-title">
            <div className="settings-card-header">
              <span className="settings-card-icon" aria-hidden="true">
                <GitBranchIcon />
              </span>
              <div>
                <h2 id="github-title">GitHub account</h2>
                <p>Used for private images from GitHub Container Registry.</p>
              </div>
            </div>
            <div className="settings-card-body">
              {isLoading ? (
                <div className="github-integration-loading" role="status">
                  <Spinner />
                  <span>Checking GitHub connection</span>
                </div>
              ) : connection?.connected ? (
                <div className="connection-state is-connected">
                  <span className="connection-dot" aria-hidden="true" />
                  <div>
                    <strong>Connected as @{connection.login}</strong>
                    <p>
                      Private <code>ghcr.io</code> images deployed by this
                      account use this GitHub connection.
                    </p>
                  </div>
                </div>
              ) : (
                <div className="connection-state">
                  <span className="connection-dot" aria-hidden="true" />
                  <div>
                    <strong>Not connected</strong>
                    <p>
                      Connect GitHub to pull private container images. Public
                      images do not need an account connection.
                    </p>
                  </div>
                </div>
              )}
            </div>
            <div className="settings-card-footer">
              <span>OAuth · read:packages</span>
              <div>
                {connection?.connected ? (
                  <>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={isLoading || isConnecting || isDisconnecting}
                      onClick={() => void connectGithub()}
                    >
                      {isConnecting ? (
                        <Spinner data-icon="inline-start" />
                      ) : (
                        <RefreshCwIcon data-icon="inline-start" />
                      )}
                      Reconnect
                    </Button>
                    <Button
                      variant="destructive"
                      size="sm"
                      disabled={isLoading || isConnecting || isDisconnecting}
                      onClick={() => void disconnectGithubAccount()}
                    >
                      {isDisconnecting && <Spinner data-icon="inline-start" />}
                      Disconnect
                    </Button>
                  </>
                ) : (
                  <Button
                    size="sm"
                    disabled={isLoading || isConnecting || isDisconnecting}
                    onClick={() => void connectGithub()}
                  >
                    {isConnecting ? (
                      <Spinner data-icon="inline-start" />
                    ) : (
                      <GitBranchIcon data-icon="inline-start" />
                    )}
                    Connect GitHub
                  </Button>
                )}
              </div>
            </div>
          </section>

          <KnotreeRegistryAccountCard />

          <p className="settings-footnote">
            These connections belong to <strong>{session?.user.email}</strong>{" "}
            and are not shared with other Knotree users. Access can be removed
            at any time.
          </p>
          {callbackStatus && (
            <Button
              variant="link"
              className="justify-self-start px-0"
              onClick={() =>
                navigate("/settings/integrations", { replace: true })
              }
            >
              Dismiss message
            </Button>
          )}
        </div>
      </div>
    </div>
  )

  if (status === "loading") {
    return <LoadingScreen />
  }

  if (!session?.workspace) {
    return (
      <main className="workspace-page">
        <header className="justify-between">
          <BrandMark compact />
          <Link
            className="text-[0.8125rem] text-muted-foreground hover:text-foreground"
            to={workspaceHref}
          >
            Back to workspace
          </Link>
        </header>
        <div className="w-full">{content}</div>
      </main>
    )
  }

  return (
    <AppShell
      workspace={session.workspace}
      user={session.user}
      nav={
        <SidebarNavItem
          label="Projects"
          icon={<FolderKanbanIcon />}
          to={workspaceHref}
        />
      }
      footerNav={
        <SidebarNavItem
          label="Integrations"
          icon={<PlugIcon />}
          active
          to="/settings/integrations"
        />
      }
      breadcrumbs={
        <>
          <Link className="app-breadcrumb-link" to={workspaceHref}>
            {session.workspace.name}
          </Link>
          <BreadcrumbSeparator />
          <span className="app-breadcrumb-current">Integrations</span>
        </>
      }
    >
      {content}
    </AppShell>
  )
}
