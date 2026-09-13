import { useEffect, useRef, useState, type FormEvent } from "react"
import { Dialog } from "@base-ui/react/dialog"
import { BoxIcon, GitBranchIcon } from "lucide-react"

import { Alert, AlertDescription } from "@/components/ui/alert"
import { AppServiceDeploymentLogs } from "@/components/app-service-deployment-logs"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldDescription,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import {
  createAppService,
  appServiceDeploymentEventsUrl,
  getGithubAuthorizationUrl,
  getGithubConnectionStatus,
  listAppServices,
} from "@/lib/resources"
import type {
  AppService,
  AppServiceDeployment,
  GithubConnectionStatus,
} from "@/lib/types"

type AppServiceCreateDialogProps = {
  workspaceSlug: string
  projectSlug: string
  appServiceCount?: number
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated: (resource: AppService) => void
}

type ImageSource = "public" | "github"

export function AppServiceCreateDialog({
  workspaceSlug,
  projectSlug,
  appServiceCount = 0,
  open,
  onOpenChange,
  onCreated,
}: AppServiceCreateDialogProps) {
  const [name, setName] = useState("App service")
  const [image, setImage] = useState("")
  const [imageSource, setImageSource] = useState<ImageSource>("public")
  const [appPort, setAppPort] = useState("3000")
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [isConnecting, setIsConnecting] = useState(false)
  const [submittedResource, setSubmittedResource] = useState<AppService | null>(
    null
  )
  const [deployment, setDeployment] = useState<AppServiceDeployment | null>(
    null
  )
  const [streamError, setStreamError] = useState<string | null>(null)
  const [githubStatus, setGithubStatus] =
    useState<GithubConnectionStatus | null>(null)
  const [githubStatusLoading, setGithubStatusLoading] = useState(false)
  const onCreatedRef = useRef(onCreated)
  const deploymentId = deployment?.id
  const deploymentIsActive = deployment?.status === "provisioning"
  const submittedResourceId = submittedResource?.id

  useEffect(() => {
    onCreatedRef.current = onCreated
  }, [onCreated])

  useEffect(() => {
    if (!open || imageSource !== "github") {
      return undefined
    }

    let active = true
    void getGithubConnectionStatus()
      .then((status) => {
        if (active) {
          setGithubStatus(status)
        }
      })
      .catch(() => {
        if (active) {
          setGithubStatus(null)
        }
      })
      .finally(() => {
        if (active) {
          setGithubStatusLoading(false)
        }
      })

    return () => {
      active = false
    }
  }, [imageSource, open])

  useEffect(() => {
    if (!open || !submittedResourceId || !deploymentId || !deploymentIsActive) {
      return undefined
    }

    if (typeof EventSource === "undefined") {
      return undefined
    }

    const source = new EventSource(
      appServiceDeploymentEventsUrl(workspaceSlug, projectSlug, deploymentId),
      { withCredentials: true }
    )
    const handleDeployment = (event: Event) => {
      try {
        const nextDeployment = JSON.parse(
          (event as MessageEvent<string>).data
        ) as AppServiceDeployment
        setDeployment(nextDeployment)
        setStreamError(null)
        if (nextDeployment.status !== "provisioning") {
          source.close()
          void listAppServices(workspaceSlug, projectSlug)
            .then((resources) => {
              const nextResource = resources.find(
                (resource) => resource.id === submittedResourceId
              )
              if (nextResource) {
                setSubmittedResource(nextResource)
                setDeployment(nextResource.deployment ?? nextDeployment)
                onCreatedRef.current(nextResource)
              }
            })
            .catch(() => {
              setStreamError(
                "Deployment finished, but the service details could not be refreshed."
              )
            })
        }
      } catch {
        setStreamError("A deployment log update could not be read.")
      }
    }
    source.addEventListener("deployment", handleDeployment)
    source.onerror = () => {
      setStreamError(
        "Live log connection interrupted. Saved logs remain available while the deployment continues."
      )
      source.close()
    }

    return () => {
      source.close()
    }
  }, [
    deploymentId,
    deploymentIsActive,
    open,
    projectSlug,
    submittedResourceId,
    workspaceSlug,
  ])

  async function connectGithub() {
    setSubmitError(null)
    setIsConnecting(true)
    try {
      const { authorizationUrl } = await getGithubAuthorizationUrl(
        window.location.pathname
      )
      window.location.assign(authorizationUrl)
    } catch (error) {
      setSubmitError(
        error instanceof ApiError
          ? error.message
          : "GitHub login is currently unavailable. Please try again."
      )
      setIsConnecting(false)
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextErrors: Record<string, string> = {}
    if (name.trim().length === 0 || name.trim().length > 80) {
      nextErrors.name = "Enter a service name between 1 and 80 characters."
    }
    if (image.trim().length === 0 || image.trim().length > 255) {
      nextErrors.image = "Paste a Docker image reference."
    }
    const parsedPort = Number(appPort)
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      nextErrors.appPort = "Use a container port between 1 and 65535."
    }
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }
    if (imageSource === "github" && !githubStatus?.connected) {
      setSubmitError("Connect GitHub before deploying a private image.")
      return
    }

    setIsSubmitting(true)
    setStreamError(null)
    try {
      const resource = await createAppService(workspaceSlug, projectSlug, {
        name: name.trim(),
        image: image.trim(),
        imageSource,
        appPort: parsedPort,
      })
      if (!resource.deployment) {
        onCreated(resource)
        onOpenChange(false)
        return
      }
      setSubmittedResource(resource)
      setDeployment(resource.deployment)
      onCreated(resource)
    } catch (error) {
      if (error instanceof ApiError) {
        setErrors(error.fields)
        setSubmitError(error.message)
      } else {
        setSubmitError("The API is currently unavailable. Please try again.")
      }
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(nextOpen) => {
        if (!isSubmitting && !isConnecting) {
          onOpenChange(nextOpen)
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Backdrop className="project-dialog-backdrop" />
        <Dialog.Popup className="project-dialog-popup project-database-dialog">
          <div className="project-dialog-header">
            <div>
              <Dialog.Title className="project-dialog-title">
                {submittedResource
                  ? "Deploying App service"
                  : "Deploy App service"}
              </Dialog.Title>
              <Dialog.Description className="project-dialog-description">
                {submittedResource
                  ? "The deployment is running in the background. Follow each Docker step and live log below."
                  : `Paste a Docker image and Knotree will run it as an isolated service for this project. New services start without a database; assign one later from Settings. (${appServiceCount}/6 services)`}
              </Dialog.Description>
            </div>
            <button
              type="button"
              className="project-dialog-close"
              aria-label="Close create app service dialog"
              onClick={() => onOpenChange(false)}
            >
              ×
            </button>
          </div>

          {!submittedResource && (
            <div className="project-dialog-note">
              <BoxIcon aria-hidden="true" />
              <span>
                Public images do not need a login. Private images are currently
                supported through GitHub Container Registry only.
              </span>
            </div>
          )}

          {!submittedResource && (
            <>
              {submitError && (
                <Alert variant="destructive">
                  <AlertDescription>{submitError}</AlertDescription>
                </Alert>
              )}

              <form
                className="project-dialog-form"
                onSubmit={handleSubmit}
                noValidate
              >
                <FieldGroup>
                  <Field data-invalid={Boolean(errors.name)}>
                    <FieldLabel htmlFor="appServiceName">
                      Service name
                    </FieldLabel>
                    <Input
                      id="appServiceName"
                      name="appServiceName"
                      value={name}
                      autoComplete="off"
                      aria-invalid={Boolean(errors.name)}
                      onChange={(event) => setName(event.target.value)}
                    />
                    {errors.name && <FieldError>{errors.name}</FieldError>}
                  </Field>
                  <Field data-invalid={Boolean(errors.image)}>
                    <FieldLabel htmlFor="appServiceImage">
                      Docker image
                    </FieldLabel>
                    <Input
                      id="appServiceImage"
                      name="appServiceImage"
                      value={image}
                      placeholder="nginx:alpine or ghcr.io/org/app:latest"
                      autoComplete="off"
                      spellCheck={false}
                      aria-invalid={Boolean(errors.image)}
                      onChange={(event) => setImage(event.target.value)}
                    />
                    <FieldDescription>
                      Include the tag when you need a specific version.
                    </FieldDescription>
                    {errors.image && <FieldError>{errors.image}</FieldError>}
                  </Field>
                  <Field data-invalid={Boolean(errors.imageSource)}>
                    <FieldLabel htmlFor="appServiceImageSource">
                      Image access
                    </FieldLabel>
                    <select
                      id="appServiceImageSource"
                      name="appServiceImageSource"
                      className="project-dialog-select"
                      value={imageSource}
                      aria-invalid={Boolean(errors.imageSource)}
                      onChange={(event) => {
                        const nextSource = event.target.value as ImageSource
                        setGithubStatusLoading(nextSource === "github")
                        if (nextSource === "public") {
                          setGithubStatus(null)
                        }
                        setImageSource(nextSource)
                      }}
                    >
                      <option value="public">Public Docker image</option>
                      <option value="github">Private GitHub image</option>
                    </select>
                    {errors.imageSource && (
                      <FieldError>{errors.imageSource}</FieldError>
                    )}
                  </Field>
                  <Field data-invalid={Boolean(errors.appPort)}>
                    <FieldLabel htmlFor="appServicePort">
                      Container port
                    </FieldLabel>
                    <Input
                      id="appServicePort"
                      name="appServicePort"
                      type="number"
                      min={1}
                      max={65535}
                      value={appPort}
                      aria-invalid={Boolean(errors.appPort)}
                      onChange={(event) => setAppPort(event.target.value)}
                    />
                    <FieldDescription>
                      The port your image listens on. Knotree assigns the public
                      port automatically.
                    </FieldDescription>
                    {errors.appPort && (
                      <FieldError>{errors.appPort}</FieldError>
                    )}
                  </Field>
                </FieldGroup>

                {imageSource === "github" && (
                  <div className="project-github-connect" role="status">
                    <div>
                      <strong>
                        {githubStatus?.connected
                          ? `Connected as @${githubStatus.login}`
                          : "GitHub connection required"}
                      </strong>
                      <span>
                        {githubStatusLoading
                          ? "Checking GitHub connection…"
                          : "Knotree uses this connection to pull private ghcr.io images."}
                      </span>
                    </div>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={githubStatusLoading || isConnecting}
                      onClick={() => void connectGithub()}
                    >
                      {isConnecting ? (
                        <Spinner data-icon="inline-start" />
                      ) : (
                        <GitBranchIcon data-icon="inline-start" />
                      )}
                      {githubStatus?.connected ? "Reconnect" : "Connect GitHub"}
                    </Button>
                  </div>
                )}

                <div className="project-dialog-actions">
                  <Button
                    type="button"
                    variant="ghost"
                    disabled={isSubmitting || isConnecting}
                    onClick={() => onOpenChange(false)}
                  >
                    Cancel
                  </Button>
                  <Button type="submit" disabled={isSubmitting || isConnecting}>
                    {isSubmitting && <Spinner data-icon="inline-start" />}
                    Deploy service
                  </Button>
                </div>
              </form>
            </>
          )}

          {submittedResource && (
            <div className="project-dialog-form">
              {deployment ? (
                <AppServiceDeploymentLogs deployment={deployment} />
              ) : (
                <Alert>
                  <AlertDescription>
                    Deployment accepted. Saved progress will appear when the
                    deployment record is available.
                  </AlertDescription>
                </Alert>
              )}
              {(streamError ||
                (typeof EventSource === "undefined" && deploymentIsActive)) && (
                <Alert variant="destructive">
                  <AlertDescription>
                    {streamError ??
                      "Live logs are unavailable in this browser. The deployment continues in the background."}
                  </AlertDescription>
                </Alert>
              )}
              <div className="project-dialog-actions">
                <Button
                  type="button"
                  variant="ghost"
                  onClick={() => onOpenChange(false)}
                >
                  Run in background
                </Button>
                {deployment?.status !== "provisioning" && (
                  <Button type="button" onClick={() => onOpenChange(false)}>
                    Done
                  </Button>
                )}
              </div>
            </div>
          )}
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
