import { useEffect, useState } from "react"
import { ContainerIcon } from "lucide-react"

import { Alert, AlertDescription } from "@/components/ui/alert"
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
      <section className="settings-card" aria-labelledby="registry-title">
        <div className="settings-card-header">
          <span className="settings-card-icon" aria-hidden="true">
            <ContainerIcon />
          </span>
          <div>
            <h2 id="registry-title">Knotree Registry</h2>
            <p>
              Your registry.knotree.com images are available in every project
              and redeploy automatically when you push a tag.
            </p>
          </div>
        </div>
        <div className="settings-card-body">
          {isLoading ? (
            <div className="github-integration-loading" role="status">
              <Spinner />
              <span>Checking Knotree Registry</span>
            </div>
          ) : account?.connected ? (
            <div className="connection-state is-connected">
              <span className="connection-dot" aria-hidden="true" />
              <div>
                <strong>
                  Linked to your Knotree account · {account.namespace}
                </strong>
                <p>
                  Nothing to connect or renew. Cloud only reads your own
                  namespace, and other users never see these images.
                </p>
              </div>
            </div>
          ) : (
            <div className="connection-state">
              <span className="connection-dot" aria-hidden="true" />
              <div>
                <strong>Unavailable</strong>
                <p>
                  Knotree Registry could not be reached. Your images will show
                  up here as soon as it is back.
                </p>
              </div>
            </div>
          )}
        </div>
      </section>
    </>
  )
}
