import { useEffect, useState } from "react"
import { CheckCircle2Icon, ContainerIcon, Link2OffIcon } from "lucide-react"

import { Alert, AlertDescription } from "@/components/ui/alert"
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { getKnotreeRegistryAccount } from "@/lib/resources"
import type { KnotreeRegistryAccountStatus } from "@/lib/types"

/**
 * Knotree Registry is part of the same Knotree account, so there is nothing to
 * connect: this card only shows the namespace Cloud deploys from.
 */
export function KnotreeRegistryAccountCard() {
  const [account, setAccount] = useState<KnotreeRegistryAccountStatus | null>(
    null
  )
  const [isLoading, setIsLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    void getKnotreeRegistryAccount()
      .then((status) => {
        if (active) setAccount(status)
      })
      .catch((reason: unknown) => {
        if (active) {
          setError(
            reason instanceof ApiError
              ? reason.message
              : "Knotree Registry status is currently unavailable."
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

  return (
    <>
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
              Your registry.knotree.com images are available in every project
              and redeploy automatically when you push a tag.
            </CardDescription>
          </div>
        </CardHeader>
        <CardContent className="github-integration-card-content">
          {isLoading ? (
            <div className="github-integration-loading" role="status">
              <Spinner />
              <span>Checking Knotree Registry</span>
            </div>
          ) : account?.connected ? (
            <div className="github-integration-connected">
              <div className="github-integration-status-dot" aria-hidden="true">
                <CheckCircle2Icon />
              </div>
              <div>
                <strong>Linked to your Knotree account · {account.namespace}</strong>
                <p>
                  Nothing to connect or renew. Cloud only reads your own
                  namespace, and other users never see these images.
                </p>
              </div>
            </div>
          ) : (
            <div className="github-integration-disconnected">
              <div className="github-integration-status-dot" aria-hidden="true">
                <Link2OffIcon />
              </div>
              <div>
                <strong>Unavailable</strong>
                <p>
                  Knotree Registry could not be reached. Your images will show
                  up here as soon as it is back.
                </p>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </>
  )
}
