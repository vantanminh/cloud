import { useEffect, useState } from "react"
import {
  ArrowLeftIcon,
  CheckCircle2Icon,
  GitBranchIcon,
  Link2OffIcon,
  RefreshCwIcon,
} from "lucide-react"
import { Link, useNavigate, useSearchParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { KnotreeRegistryAccountCard } from "@/components/knotree-registry-account-card"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import {
  disconnectGithub,
  getGithubAuthorizationUrl,
  getGithubConnectionStatus,
} from "@/lib/resources"
import type { GithubConnectionStatus } from "@/lib/types"

export function GitHubIntegrationPage() {
  const { session } = useAuth()
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

  return (
    <main className="github-integration-page">
      <header className="github-integration-header">
        <div className="github-integration-header-inner">
          <div className="github-integration-brand">
            <span className="github-integration-brand-mark" aria-hidden="true">
              <GitBranchIcon />
            </span>
            <div>
              <p className="github-integration-eyebrow">Account settings</p>
              <p className="github-integration-heading">Integrations</p>
            </div>
          </div>
          <Link className="github-integration-back" to={workspaceHref}>
            <ArrowLeftIcon aria-hidden="true" />
            Back to workspace
          </Link>
        </div>
      </header>

      <section className="github-integration-content">
        <div className="github-integration-intro">
          <p className="github-integration-eyebrow">Developer access</p>
          <h1>Integrations</h1>
          <p>
            Connect GitHub and Knotree Registry once and use their image access
            across every project you deploy on Knotree.
          </p>
        </div>

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

        <Card className="github-integration-card">
          <CardHeader className="github-integration-card-header">
            <span className="github-integration-card-icon" aria-hidden="true">
              <GitBranchIcon />
            </span>
            <div>
              <CardTitle>GitHub account</CardTitle>
              <CardDescription>
                Used for private images from GitHub Container Registry.
              </CardDescription>
            </div>
          </CardHeader>
          <CardContent className="github-integration-card-content">
            {isLoading ? (
              <div className="github-integration-loading" role="status">
                <Spinner />
                <span>Checking GitHub connection</span>
              </div>
            ) : connection?.connected ? (
              <div className="github-integration-connected">
                <div
                  className="github-integration-status-dot"
                  aria-hidden="true"
                >
                  <CheckCircle2Icon />
                </div>
                <div>
                  <strong>Connected as @{connection.login}</strong>
                  <p>
                    Private <code>ghcr.io</code> images deployed by this account
                    use this GitHub connection.
                  </p>
                </div>
              </div>
            ) : (
              <div className="github-integration-disconnected">
                <div
                  className="github-integration-status-dot"
                  aria-hidden="true"
                >
                  <Link2OffIcon />
                </div>
                <div>
                  <strong>Not connected</strong>
                  <p>
                    Connect GitHub to pull private container images. Public
                    images do not need an account connection.
                  </p>
                </div>
              </div>
            )}
          </CardContent>
          <CardFooter className="github-integration-card-footer">
            {connection?.connected ? (
              <>
                <Button
                  variant="outline"
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
                  disabled={isLoading || isConnecting || isDisconnecting}
                  onClick={() => void disconnectGithubAccount()}
                >
                  {isDisconnecting && <Spinner data-icon="inline-start" />}
                  Disconnect
                </Button>
              </>
            ) : (
              <Button
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
          </CardFooter>
        </Card>

        <KnotreeRegistryAccountCard
          callbackStatus={searchParams.get("registry")}
        />

        <p className="github-integration-footnote">
          These connections belong to <strong>{session?.user.email}</strong> and
          are not shared with other Knotree users. Access can be removed at any
          time.
        </p>
        {(callbackStatus || searchParams.get("registry")) && (
          <Button
            variant="link"
            className="github-integration-dismiss"
            onClick={() =>
              navigate("/settings/integrations", { replace: true })
            }
          >
            Dismiss message
          </Button>
        )}
      </section>
    </main>
  )
}
