import { useEffect, useState } from "react"
import {
  CheckCircle2Icon,
  ContainerIcon,
  Link2OffIcon,
  RefreshCwIcon,
} from "lucide-react"

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
  disconnectKnotreeRegistryAccount,
  getKnotreeRegistryAccount,
  isKnotreeRegistryAuthorizationUrl,
  startKnotreeRegistryAccountConsent,
} from "@/lib/resources"
import type { KnotreeRegistryAccountStatus } from "@/lib/types"

/** Starts the one-time, SSO-bound Registry consent and leaves for Registry. */
export async function connectKnotreeRegistryAccount(returnTo: string) {
  const { authorizationUrl } =
    await startKnotreeRegistryAccountConsent(returnTo)
  if (!isKnotreeRegistryAuthorizationUrl(authorizationUrl)) {
    throw new Error("Registry returned an invalid authorization URL.")
  }
  window.location.assign(authorizationUrl)
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof ApiError || error instanceof Error
    ? error.message
    : fallback
}

export function KnotreeRegistryAccountCard({
  callbackStatus,
}: {
  callbackStatus: string | null
}) {
  const [account, setAccount] = useState<KnotreeRegistryAccountStatus | null>(
    null
  )
  const [isLoading, setIsLoading] = useState(true)
  const [isConnecting, setIsConnecting] = useState(false)
  const [isDisconnecting, setIsDisconnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    void getKnotreeRegistryAccount()
      .then((status) => {
        if (active) setAccount(status)
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(
            errorMessage(
              reason,
              "Knotree Registry status is currently unavailable."
            )
          )
        }
      })
      .finally(() => {
        if (active) setIsLoading(false)
      })
    return () => {
      active = false
    }
  }, [])

  async function connect() {
    setError(null)
    setIsConnecting(true)
    try {
      await connectKnotreeRegistryAccount("/settings/integrations")
    } catch (reason) {
      setError(errorMessage(reason, "Registry authorization failed."))
      setIsConnecting(false)
    }
  }

  async function disconnect() {
    setError(null)
    setIsDisconnecting(true)
    try {
      await disconnectKnotreeRegistryAccount()
      setAccount((current) =>
        current
          ? {
              ...current,
              connected: false,
              namespace: null,
              expiresAt: null,
              expired: false,
            }
          : current
      )
      setNotice(
        "Knotree Registry was disconnected. Auto-deploys from it are turned off."
      )
    } catch (reason) {
      setError(errorMessage(reason, "Registry could not be disconnected."))
    } finally {
      setIsDisconnecting(false)
    }
  }

  const callbackMessage =
    callbackStatus === "connected"
      ? "Knotree Registry is connected. Import your images from any project."
      : callbackStatus === "denied"
        ? "Registry access was not granted."
        : callbackStatus === "error"
          ? "Knotree Registry could not be connected. Try again."
          : null
  const busy = isLoading || isConnecting || isDisconnecting
  const expiresOn = account?.expiresAt
    ? new Date(account.expiresAt).toLocaleDateString()
    : null

  return (
    <>
      {callbackMessage && (
        <Alert
          variant={callbackStatus === "connected" ? "default" : "destructive"}
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
      {error && (
        <Alert variant="destructive">
          <AlertDescription>{error}</AlertDescription>
        </Alert>
      )}
      <Card className="github-integration-card">
        <CardHeader className="github-integration-card-header">
          <span className="github-integration-card-icon" aria-hidden="true">
            <ContainerIcon />
          </span>
          <div>
            <CardTitle>Knotree Registry</CardTitle>
            <CardDescription>
              Deploy your own registry.knotree.com images and redeploy
              automatically when you push a tag.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="github-integration-card-content">
          {isLoading ? (
            <div className="github-integration-loading" role="status">
              <Spinner />
              <span>Checking Knotree Registry connection</span>
            </div>
          ) : account?.connected ? (
            <div className="github-integration-connected">
              <div className="github-integration-status-dot" aria-hidden="true">
                <CheckCircle2Icon />
              </div>
              <div>
                <strong>
                  {account.expired
                    ? `Expired · ${account.namespace}`
                    : `Connected as ${account.namespace}`}
                </strong>
                <p>
                  {account.expired
                    ? "Reconnect to keep pulling images and auto-deploying."
                    : `Pull-only access to your own namespace${
                        expiresOn ? ` until ${expiresOn}` : ""
                      }. Other users never see these images.`}
                </p>
              </div>
            </div>
          ) : (
            <div className="github-integration-disconnected">
              <div className="github-integration-status-dot" aria-hidden="true">
                <Link2OffIcon />
              </div>
              <div>
                <strong>Not connected</strong>
                <p>
                  {account && !account.consentReady
                    ? "Sign in with Knotree Accounts to connect Registry."
                    : "Connect once to pick images from your Registry namespace in every project."}
                </p>
              </div>
            </div>
          )}
        </CardContent>
        <CardFooter className="github-integration-card-footer">
          {account?.connected ? (
            <>
              <Button
                variant="outline"
                disabled={busy}
                onClick={() => void connect()}
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
                disabled={busy}
                onClick={() => void disconnect()}
              >
                {isDisconnecting && <Spinner data-icon="inline-start" />}
                Disconnect
              </Button>
            </>
          ) : (
            <Button
              disabled={busy || account?.consentReady === false}
              onClick={() => void connect()}
            >
              {isConnecting ? (
                <Spinner data-icon="inline-start" />
              ) : (
                <ContainerIcon data-icon="inline-start" />
              )}
              Connect Knotree Registry
            </Button>
          )}
        </CardFooter>
      </Card>
    </>
  )
}
