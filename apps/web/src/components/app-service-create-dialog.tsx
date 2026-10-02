import { useEffect, useRef, useState, type FormEvent } from "react"
import { Dialog } from "@base-ui/react/dialog"
import { BoxIcon, FileCodeIcon, GitBranchIcon } from "lucide-react"

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
import { Textarea } from "@/components/ui/textarea"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { registryAuthorizationTarget } from "@/lib/registry-consent"
import {
  type CreateAppServiceInput,
  createAppService,
  createKnotreeRegistryConnection,
  startKnotreeRegistryConsent,
  appServiceDeploymentEventsUrl,
  listKnotreeRegistryConnections,
  getGithubAuthorizationUrl,
  getGithubConnectionStatus,
  listAppServices,
} from "@/lib/resources"
import type {
  AppService,
  AppServiceDeployment,
  GithubConnectionStatus,
  KnotreeRegistryConnection,
} from "@/lib/types"

type AppServiceCreateDialogProps = {
  workspaceId: string
  projectSlug: string
  appServiceCount?: number
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated: (resource: AppService) => void
}

type ServiceKind = "docker" | "html"
type ImageSource = "public" | "github" | "knotree_registry"
type HtmlSource = "paste" | "github"
const NEW_REGISTRY_CONNECTION = "new"

export function AppServiceCreateDialog({
  workspaceId,
  projectSlug,
  appServiceCount = 0,
  open,
  onOpenChange,
  onCreated,
}: AppServiceCreateDialogProps) {
  const [kind, setKind] = useState<ServiceKind>("docker")
  const [name, setName] = useState("App service")
  const [image, setImage] = useState("")
  const [imageSource, setImageSource] = useState<ImageSource>("public")
  const [htmlSource, setHtmlSource] = useState<HtmlSource>("paste")
  const [indexHtml, setIndexHtml] = useState("")
  const [githubRepo, setGithubRepo] = useState("")
  const [githubBranch, setGithubBranch] = useState("")
  const [pageSlug, setPageSlug] = useState("")
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
  const [registryUsername, setRegistryUsername] = useState("")
  const [registryToken, setRegistryToken] = useState("")
  const [registryConnections, setRegistryConnections] = useState<
    KnotreeRegistryConnection[]
  >([])
  const [registryConnectionChoice, setRegistryConnectionChoice] = useState(
    NEW_REGISTRY_CONNECTION
  )
  const [registryConsentReady, setRegistryConsentReady] = useState(false)
  const [registryConsentBusy, setRegistryConsentBusy] = useState(false)
  const [registryAutoDeployReady, setRegistryAutoDeployReady] = useState(false)
  const [registryAutoDeploy, setRegistryAutoDeploy] = useState(false)
  const [registryConnectionsProjectKey, setRegistryConnectionsProjectKey] =
    useState("")
  const [registryStatusError, setRegistryStatusError] = useState<string | null>(
    null
  )
  const onCreatedRef = useRef(onCreated)
  const deploymentId = deployment?.id
  const deploymentIsActive = deployment?.status === "provisioning"
  const submittedResourceId = submittedResource?.id
  const needsGithub =
    (kind === "docker" && imageSource === "github") ||
    (kind === "html" && htmlSource === "github")
  const needsKnotreeRegistry =
    kind === "docker" && imageSource === "knotree_registry"
  const registryProjectKey = `${workspaceId}:${projectSlug}`
  const registryStatusLoading =
    needsKnotreeRegistry && registryConnectionsProjectKey !== registryProjectKey
  const currentRegistryConnections =
    registryConnectionsProjectKey === registryProjectKey
      ? registryConnections
      : []
  const registryRepository = needsKnotreeRegistry
    ? registryRepositoryFromImage(image)
    : ""
  const matchingRegistryConnections = currentRegistryConnections.filter(
    (connection) => connection.repository === registryRepository
  )
  const registryChoice = matchingRegistryConnections.some(
    (connection) => connection.id === registryConnectionChoice
  )
    ? registryConnectionChoice
    : NEW_REGISTRY_CONNECTION

  useEffect(() => {
    onCreatedRef.current = onCreated
  }, [onCreated])

  useEffect(() => {
    if (!open || !needsGithub) {
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
  }, [needsGithub, open])

  useEffect(() => {
    if (!open || !needsKnotreeRegistry) {
      return undefined
    }

    let active = true
    void listKnotreeRegistryConnections(workspaceId, projectSlug)
      .then((result) => {
        if (active) {
          setRegistryConnections(result.connections)
          setRegistryConsentReady(result.consentReady === true)
          setRegistryAutoDeployReady(result.autoDeployReady)
          if (!result.autoDeployReady) setRegistryAutoDeploy(false)
          setRegistryConnectionsProjectKey(registryProjectKey)
          setRegistryStatusError(null)
        }
      })
      .catch(() => {
        if (active) {
          setRegistryConnections([])
          setRegistryConsentReady(false)
          setRegistryStatusError(
            "Saved Registry connections could not be loaded. You can still connect a new repository."
          )
          setRegistryAutoDeployReady(false)
          setRegistryAutoDeploy(false)
          setRegistryConnectionsProjectKey(registryProjectKey)
        }
      })

    return () => {
      active = false
    }
  }, [needsKnotreeRegistry, open, projectSlug, registryProjectKey, workspaceId])

  useEffect(() => {
    if (!open || !submittedResourceId || !deploymentId || !deploymentIsActive) {
      return undefined
    }

    if (typeof EventSource === "undefined") {
      return undefined
    }

    const source = new EventSource(
      appServiceDeploymentEventsUrl(workspaceId, projectSlug, deploymentId),
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
          void listAppServices(workspaceId, projectSlug)
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
    workspaceId,
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
    if (kind === "docker") {
      if (image.trim().length === 0 || image.trim().length > 255) {
        nextErrors.image = "Paste a Docker image reference."
      }
      const parsedPort = Number(appPort)
      if (
        !Number.isInteger(parsedPort) ||
        parsedPort < 1 ||
        parsedPort > 65535
      ) {
        nextErrors.appPort = "Use a container port between 1 and 65535."
      }
      if (needsKnotreeRegistry) {
        if (!registryRepository) {
          nextErrors.image =
            "Use registry.knotree.com/repository:tag with an explicit tag."
        }
        if (registryChoice === NEW_REGISTRY_CONNECTION) {
          if (!registryUsername.trim()) {
            nextErrors.username = "Enter your Knotree Registry username."
          }
          if (!registryToken.trim()) {
            nextErrors.token = "Enter a pull-only Registry access token."
          }
        } else if (
          !matchingRegistryConnections.some(
            (connection) => connection.id === registryChoice
          )
        ) {
          nextErrors.registryConnectionId =
            "Connect a Registry token for this repository."
        }
      }
    } else {
      const slug = htmlPageSlug(pageSlug || name)
      if (!/^[a-z0-9]([a-z0-9-]{0,46}[a-z0-9])?$/.test(slug)) {
        nextErrors.pageSlug =
          "Choose a unique suffix of lowercase letters, numbers, and hyphens."
      }
      if (htmlSource === "paste" && indexHtml.trim().length === 0) {
        nextErrors.indexHtml = "Paste the contents of index.html."
      }
      if (htmlSource === "github" && githubRepo.trim().length === 0) {
        nextErrors.githubRepo = "Enter a GitHub owner/repo or repository URL."
      }
    }
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }
    if (needsGithub && !githubStatus?.connected) {
      setSubmitError(
        "Connect GitHub before deploying from a GitHub repository."
      )
      return
    }
    if (
      needsKnotreeRegistry &&
      registryAutoDeploy &&
      !registryAutoDeployReady
    ) {
      setSubmitError(
        "Automatic Registry deploys are not configured on this Cloud installation yet."
      )
      return
    }
    if (needsKnotreeRegistry && registryStatusLoading) {
      setSubmitError("Loading saved Knotree Registry connections…")
      return
    }

    setIsSubmitting(true)
    setStreamError(null)
    try {
      let dockerInput: CreateAppServiceInput | undefined =
        kind === "docker"
          ? {
              name: name.trim(),
              image: image.trim(),
              imageSource,
              appPort: Number(appPort),
            }
          : undefined
      if (needsKnotreeRegistry) {
        let registryConnectionId = registryChoice
        if (registryChoice === NEW_REGISTRY_CONNECTION) {
          const connection = await createKnotreeRegistryConnection(
            workspaceId,
            projectSlug,
            {
              username: registryUsername.trim(),
              token: registryToken.trim(),
              repository: registryRepository,
            }
          )
          registryConnectionId = connection.id
          setRegistryConnections((connections) => [connection, ...connections])
          setRegistryConnectionChoice(connection.id)
          setRegistryToken("")
        }
        if (dockerInput) {
          dockerInput = {
            ...dockerInput,
            registryConnectionId,
            autoDeploy: registryAutoDeploy,
          }
        }
      }
      const resource = await createAppService(
        workspaceId,
        projectSlug,
        kind === "html"
          ? {
              name: name.trim(),
              imageSource: htmlSource === "github" ? "html_github" : "html",
              pageSlug: htmlPageSlug(pageSlug || name),
              indexHtml: htmlSource === "paste" ? indexHtml : undefined,
              githubRepo:
                htmlSource === "github" ? githubRepo.trim() : undefined,
              githubBranch:
                htmlSource === "github" && githubBranch.trim()
                  ? githubBranch.trim()
                  : undefined,
              autoDeploy: htmlSource === "github",
            }
          : dockerInput!
      )
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
                  : kind === "html"
                    ? `Host a static HTML page from a pasted index.html or a GitHub Pages-style repo. Public hostnames start with page- and stay unique. (${appServiceCount}/6 services)`
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
              {kind === "html" ? (
                <FileCodeIcon aria-hidden="true" />
              ) : (
                <BoxIcon aria-hidden="true" />
              )}
              <span>
                {kind === "html"
                  ? "Knotree injects analytics, serves CSS/JS/subfolders like GitHub Pages, and sets Cloudflare cache headers on static assets."
                  : needsKnotreeRegistry
                    ? "Use a pull-only Knotree Registry token scoped to this image repository. Cloud stores it encrypted and keeps it out of your service container."
                    : "Public images do not need a login. Private images can use GitHub Container Registry or Knotree Registry."}
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
                  <Field>
                    <FieldLabel htmlFor="appServiceKind">
                      Service type
                    </FieldLabel>
                    <select
                      id="appServiceKind"
                      name="appServiceKind"
                      className="project-dialog-select"
                      value={kind}
                      onChange={(event) => {
                        const nextKind = event.target.value as ServiceKind
                        setKind(nextKind)
                        setGithubStatusLoading(
                          nextKind === "html"
                            ? htmlSource === "github"
                            : imageSource === "github"
                        )
                        if (nextKind === "html") {
                          setName((current) =>
                            current === "App service" ? "HTML page" : current
                          )
                        }
                      }}
                    >
                      <option value="docker">Docker app service</option>
                      <option value="html">HTML page</option>
                    </select>
                  </Field>
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
                  {kind === "docker" ? (
                    <>
                      <Field data-invalid={Boolean(errors.image)}>
                        <FieldLabel htmlFor="appServiceImage">
                          Docker image
                        </FieldLabel>
                        <Input
                          id="appServiceImage"
                          name="appServiceImage"
                          value={image}
                          placeholder="nginx:alpine or registry.knotree.com/team/api:production"
                          autoComplete="off"
                          spellCheck={false}
                          aria-invalid={Boolean(errors.image)}
                          onChange={(event) => setImage(event.target.value)}
                        />
                        <FieldDescription>
                          Include the tag when you need a specific version.
                        </FieldDescription>
                        {errors.image && (
                          <FieldError>{errors.image}</FieldError>
                        )}
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
                          <option value="knotree_registry">
                            Knotree Registry
                          </option>
                        </select>
                        {errors.imageSource && (
                          <FieldError>{errors.imageSource}</FieldError>
                        )}
                      </Field>
                      {needsKnotreeRegistry && (
                        <>
                          <Field
                            data-invalid={Boolean(errors.registryConnectionId)}
                          >
                            <FieldLabel htmlFor="knotreeRegistryConnection">
                              Knotree Registry connection
                            </FieldLabel>
                            <select
                              id="knotreeRegistryConnection"
                              name="knotreeRegistryConnection"
                              className="project-dialog-select"
                              value={registryChoice}
                              aria-invalid={Boolean(
                                errors.registryConnectionId
                              )}
                              onChange={(event) =>
                                setRegistryConnectionChoice(event.target.value)
                              }
                            >
                              <option value={NEW_REGISTRY_CONNECTION}>
                                Connect Knotree Registry
                              </option>
                              {matchingRegistryConnections.map((connection) => (
                                <option
                                  key={connection.id}
                                  value={connection.id}
                                >
                                  {connection.registryHost}/
                                  {connection.repository} · @
                                  {connection.username}
                                </option>
                              ))}
                            </select>
                            <FieldDescription>
                              {registryStatusLoading
                                ? "Loading saved Registry connections…"
                                : registryRepository
                                  ? `This connection needs pull access to repository:${registryRepository}:pull.`
                                  : "Enter a tagged image above to select or create a repository connection."}
                            </FieldDescription>
                            {errors.registryConnectionId && (
                              <FieldError>
                                {errors.registryConnectionId}
                              </FieldError>
                            )}
                          </Field>
                          {registryConsentReady &&
                            registryChoice === NEW_REGISTRY_CONNECTION && (
                              <Field>
                                <Button
                                  type="button"
                                  variant="outline"
                                  disabled={
                                    !registryRepository ||
                                    registryConsentBusy ||
                                    isSubmitting
                                  }
                                  onClick={async () => {
                                    setRegistryConsentBusy(true)
                                    setSubmitError(null)
                                    try {
                                      const result =
                                        await startKnotreeRegistryConsent(
                                          workspaceId,
                                          projectSlug,
                                          registryRepository
                                        )
                                      window.location.assign(
                                        registryAuthorizationTarget(
                                          result.authorizationUrl
                                        )
                                      )
                                    } catch (reason) {
                                      setSubmitError(
                                        reason instanceof Error
                                          ? reason.message
                                          : "Registry authorization failed."
                                      )
                                      setRegistryConsentBusy(false)
                                    }
                                  }}
                                >
                                  {registryConsentBusy
                                    ? "Connecting…"
                                    : "Authorize Registry pull access"}
                                </Button>
                                <FieldDescription>
                                  Review access on Registry, then return to this
                                  project to select the saved connection.
                                </FieldDescription>
                              </Field>
                            )}
                          {registryChoice === NEW_REGISTRY_CONNECTION && (
                            <>
                              <Field data-invalid={Boolean(errors.username)}>
                                <FieldLabel htmlFor="knotreeRegistryUsername">
                                  Registry username
                                </FieldLabel>
                                <Input
                                  id="knotreeRegistryUsername"
                                  name="knotreeRegistryUsername"
                                  value={registryUsername}
                                  autoComplete="username"
                                  aria-invalid={Boolean(errors.username)}
                                  onChange={(event) =>
                                    setRegistryUsername(event.target.value)
                                  }
                                />
                                {errors.username && (
                                  <FieldError>{errors.username}</FieldError>
                                )}
                              </Field>
                              <Field data-invalid={Boolean(errors.token)}>
                                <FieldLabel htmlFor="knotreeRegistryToken">
                                  Pull-only access token
                                </FieldLabel>
                                <Input
                                  id="knotreeRegistryToken"
                                  name="knotreeRegistryToken"
                                  type="password"
                                  value={registryToken}
                                  autoComplete="new-password"
                                  aria-invalid={Boolean(errors.token)}
                                  onChange={(event) =>
                                    setRegistryToken(event.target.value)
                                  }
                                />
                                <FieldDescription>
                                  Create a token with only pull permission for
                                  this repository. Cloud encrypts it; it is
                                  never passed to your container.
                                </FieldDescription>
                                {errors.token && (
                                  <FieldError>{errors.token}</FieldError>
                                )}
                              </Field>
                            </>
                          )}
                          {registryStatusError && (
                            <p
                              className="project-dialog-description"
                              role="status"
                            >
                              {registryStatusError}
                            </p>
                          )}
                          <label className="project-registry-auto-deploy">
                            <input
                              type="checkbox"
                              checked={registryAutoDeploy}
                              disabled={
                                !registryAutoDeployReady ||
                                registryStatusLoading ||
                                isSubmitting
                              }
                              onChange={(event) =>
                                setRegistryAutoDeploy(event.target.checked)
                              }
                            />
                            <span>
                              <strong>Auto-deploy new image digests</strong>
                              <small>
                                {registryAutoDeployReady
                                  ? "Signed tag update events will deploy this tag by digest."
                                  : "Available after the Cloud operator configures the Registry webhook."}
                              </small>
                            </span>
                          </label>
                        </>
                      )}
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
                          The port your image listens on. Knotree assigns the
                          public port automatically.
                        </FieldDescription>
                        {errors.appPort && (
                          <FieldError>{errors.appPort}</FieldError>
                        )}
                      </Field>
                    </>
                  ) : (
                    <>
                      <Field data-invalid={Boolean(errors.htmlSource)}>
                        <FieldLabel htmlFor="htmlSource">
                          HTML source
                        </FieldLabel>
                        <select
                          id="htmlSource"
                          name="htmlSource"
                          className="project-dialog-select"
                          value={htmlSource}
                          onChange={(event) => {
                            const nextSource = event.target.value as HtmlSource
                            setGithubStatusLoading(nextSource === "github")
                            if (nextSource === "paste") {
                              setGithubStatus(null)
                            }
                            setHtmlSource(nextSource)
                          }}
                        >
                          <option value="paste">Paste index.html</option>
                          <option value="github">GitHub HTML repository</option>
                        </select>
                        <FieldDescription>
                          Repositories are published like GitHub Pages:
                          index.html is the root, and CSS, JS, and folders are
                          included.
                        </FieldDescription>
                      </Field>
                      <Field data-invalid={Boolean(errors.pageSlug)}>
                        <FieldLabel htmlFor="htmlPageSlug">
                          Public domain
                        </FieldLabel>
                        <Input
                          id="htmlPageSlug"
                          name="htmlPageSlug"
                          value={pageSlug}
                          placeholder="docs"
                          autoComplete="off"
                          spellCheck={false}
                          aria-invalid={Boolean(errors.pageSlug)}
                          onChange={(event) => setPageSlug(event.target.value)}
                        />
                        <FieldDescription>
                          Hostname will be page-
                          {htmlPageSlug(pageSlug || name) || "your-name"}
                          .knotree.org and must be unique.
                        </FieldDescription>
                        {errors.pageSlug && (
                          <FieldError>{errors.pageSlug}</FieldError>
                        )}
                      </Field>
                      {htmlSource === "paste" ? (
                        <Field data-invalid={Boolean(errors.indexHtml)}>
                          <FieldLabel htmlFor="htmlIndex">
                            index.html
                          </FieldLabel>
                          <Textarea
                            id="htmlIndex"
                            name="htmlIndex"
                            value={indexHtml}
                            rows={12}
                            spellCheck={false}
                            aria-invalid={Boolean(errors.indexHtml)}
                            placeholder="<!doctype html>..."
                            onChange={(event) =>
                              setIndexHtml(event.target.value)
                            }
                          />
                          {errors.indexHtml && (
                            <FieldError>{errors.indexHtml}</FieldError>
                          )}
                        </Field>
                      ) : (
                        <>
                          <Field data-invalid={Boolean(errors.githubRepo)}>
                            <FieldLabel htmlFor="htmlGithubRepo">
                              GitHub repository
                            </FieldLabel>
                            <Input
                              id="htmlGithubRepo"
                              name="htmlGithubRepo"
                              value={githubRepo}
                              placeholder="acme/docs-site"
                              autoComplete="off"
                              spellCheck={false}
                              aria-invalid={Boolean(errors.githubRepo)}
                              onChange={(event) =>
                                setGithubRepo(event.target.value)
                              }
                            />
                            {errors.githubRepo && (
                              <FieldError>{errors.githubRepo}</FieldError>
                            )}
                          </Field>
                          <Field>
                            <FieldLabel htmlFor="htmlGithubBranch">
                              Branch (optional)
                            </FieldLabel>
                            <Input
                              id="htmlGithubBranch"
                              name="htmlGithubBranch"
                              value={githubBranch}
                              placeholder="main"
                              autoComplete="off"
                              spellCheck={false}
                              onChange={(event) =>
                                setGithubBranch(event.target.value)
                              }
                            />
                            <FieldDescription>
                              New pushes to this branch are deployed
                              automatically.
                            </FieldDescription>
                          </Field>
                        </>
                      )}
                    </>
                  )}
                </FieldGroup>

                {needsGithub && (
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
                          : kind === "html"
                            ? "Reconnect GitHub if you connected earlier for images only. HTML repos need the repo scope so Knotree can clone and auto-deploy on push."
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
                  <Button
                    type="submit"
                    disabled={
                      isSubmitting ||
                      isConnecting ||
                      (needsKnotreeRegistry && registryStatusLoading)
                    }
                  >
                    {isSubmitting && <Spinner data-icon="inline-start" />}
                    {needsKnotreeRegistry &&
                    registryChoice === NEW_REGISTRY_CONNECTION
                      ? "Connect Knotree Registry & deploy"
                      : "Deploy service"}
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

function htmlPageSlug(value: string) {
  const trimmed = value.trim().replace(/^page-/i, "")
  return trimmed
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 48)
}

function registryRepositoryFromImage(value: string) {
  const image = value.trim()
  const prefix = "registry.knotree.com/"
  if (!image.toLowerCase().startsWith(prefix) || image.includes("@")) {
    return ""
  }
  const reference = image.slice(prefix.length)
  const separator = reference.lastIndexOf(":")
  const lastSlash = reference.lastIndexOf("/")
  if (separator <= lastSlash || separator === reference.length - 1) {
    return ""
  }
  const repository = reference.slice(0, separator)
  const tag = reference.slice(separator + 1)
  return repository && !tag.includes("/") ? repository : ""
}
