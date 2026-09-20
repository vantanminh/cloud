import { useEffect, useState, type FormEvent, type ReactNode } from "react"
import {
  ArrowUpRightIcon,
  CheckIcon,
  FolderKanbanIcon,
  LogOutIcon,
  PlusIcon,
  Settings2Icon,
} from "lucide-react"
import { Link, Navigate, useNavigate, useParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { BrandMark } from "@/components/brand-mark"
import { ProjectCreateDialog } from "@/components/project-create-dialog"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { listProjects } from "@/lib/projects"
import type { Project, Workspace } from "@/lib/types"

export function NewWorkspacePage() {
  const navigate = useNavigate()
  const { createWorkspace, session } = useAuth()
  const [name, setName] = useState("")
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [createdWorkspace, setCreatedWorkspace] = useState<Workspace | null>(
    null
  )

  if (session?.workspace && !createdWorkspace) {
    return <Navigate to={`/workspace/${session.workspace.id}`} replace />
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextErrors: Record<string, string> = {}
    if (name.trim().length === 0 || name.trim().length > 80) {
      nextErrors.name = "Enter a name between 1 and 80 characters."
    }
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }

    setIsSubmitting(true)
    try {
      const workspace = await createWorkspace({ name: name.trim() })
      setCreatedWorkspace(workspace)
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

  if (createdWorkspace) {
    return (
      <WorkspaceCreated
        workspace={createdWorkspace}
        onContinue={() => navigate(`/workspace/${createdWorkspace.id}`)}
      />
    )
  }

  return (
    <WorkspaceFrame>
      <div className="workspace-content">
        <div className="workspace-stepper" aria-label="Step 1 of 2">
          <span className="workspace-stepper-active" />
          <span />
        </div>
        <div className="flex flex-col gap-3">
          <h1 className="font-heading text-3xl font-semibold tracking-[-0.04em] text-foreground sm:text-4xl">
            Create your first workspace
          </h1>
          <p className="text-base leading-7 text-muted-foreground">
            A workspace is where your ideas come together.
          </p>
        </div>

        {submitError && (
          <Alert variant="destructive">
            <AlertDescription>{submitError}</AlertDescription>
          </Alert>
        )}

        <form
          className="flex flex-col gap-6"
          onSubmit={handleSubmit}
          noValidate
        >
          <FieldGroup>
            <Field data-invalid={Boolean(errors.name)}>
              <FieldLabel htmlFor="workspaceName">Workspace name</FieldLabel>
              <Input
                id="workspaceName"
                name="workspaceName"
                value={name}
                placeholder="Acme Studio"
                autoComplete="organization"
                aria-invalid={Boolean(errors.name)}
                onChange={(event) => setName(event.target.value)}
              />
              {errors.name && <FieldError>{errors.name}</FieldError>}
            </Field>
          </FieldGroup>

          <Button
            type="submit"
            size="lg"
            disabled={isSubmitting}
            className="h-11 w-full text-sm"
          >
            {isSubmitting && <Spinner data-icon="inline-start" />}
            Create workspace
          </Button>
        </form>

        <p className="text-center text-sm text-muted-foreground">
          Projects and services can be added any time.
        </p>
      </div>
    </WorkspaceFrame>
  )
}

export function WorkspacePage() {
  const { session, signOut } = useAuth()
  const { workspaceId: routeWorkspaceId } = useParams()
  const navigate = useNavigate()
  const workspace = session?.workspace
  const [projects, setProjects] = useState<Project[] | null>(null)
  const [projectsError, setProjectsError] = useState<string | null>(null)
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const workspaceId = workspace?.id

  useEffect(() => {
    if (!workspaceId) {
      return undefined
    }

    let active = true
    void listProjects(workspaceId)
      .then((nextProjects) => {
        if (active) {
          setProjects(nextProjects)
        }
      })
      .catch((error: unknown) => {
        if (!active) {
          return
        }
        setProjects([])
        setProjectsError(
          error instanceof ApiError
            ? error.message
            : "The API is currently unavailable. Please try again."
        )
      })

    return () => {
      active = false
    }
  }, [workspaceId])

  async function handleSignOut() {
    try {
      await signOut()
    } finally {
      navigate("/login", { replace: true })
    }
  }

  if (!session || !workspace || workspace.id !== routeWorkspaceId) {
    return null
  }

  const currentWorkspaceId = workspace.id

  function handleProjectCreated(project: Project) {
    setProjects((currentProjects) =>
      currentProjects ? [...currentProjects, project] : [project]
    )
    setIsCreateOpen(false)
    navigate(`/workspace/${currentWorkspaceId}/project/${project.slug}`)
  }

  return (
    <main className="workspace-projects-page">
      <header className="workspace-projects-header">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-5 py-5 sm:px-8">
          <BrandMark compact />
          <div className="flex items-center gap-4">
            <span className="hidden text-sm text-muted-foreground sm:inline">
              {session.user.email}
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={() => setIsCreateOpen(true)}
            >
              <PlusIcon data-icon="inline-start" />
              New project
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => navigate("/settings/integrations")}
            >
              <Settings2Icon data-icon="inline-start" />
              Settings
            </Button>
            <Button variant="ghost" size="sm" onClick={handleSignOut}>
              <LogOutIcon data-icon="inline-start" />
              Sign out
            </Button>
          </div>
        </div>
      </header>
      <section className="workspace-projects-content">
        <div className="workspace-projects-intro">
          <div className="flex max-w-xl flex-col gap-3">
            <p className="workspace-projects-eyebrow">Workspace</p>
            <h1 className="font-heading text-4xl font-semibold tracking-[-0.04em] text-foreground sm:text-5xl">
              Welcome to {workspace.name}
            </h1>
            <p className="text-lg leading-8 text-muted-foreground">
              Create a project to map its services, data, and infrastructure in
              one place.
            </p>
          </div>
          <Button size="lg" onClick={() => setIsCreateOpen(true)}>
            <PlusIcon data-icon="inline-start" />
            New project
          </Button>
        </div>

        {projectsError && (
          <Alert variant="destructive">
            <AlertDescription>{projectsError}</AlertDescription>
          </Alert>
        )}

        {projects === null ? (
          <div className="workspace-projects-loading" role="status">
            <Spinner />
            <span>Loading projects</span>
          </div>
        ) : projects.length === 0 ? (
          <section
            className="workspace-empty-projects"
            aria-labelledby="empty-projects-title"
          >
            <div className="workspace-empty-icon" aria-hidden="true">
              <FolderKanbanIcon />
            </div>
            <div className="flex max-w-md flex-col gap-3">
              <h2
                id="empty-projects-title"
                className="text-2xl font-semibold tracking-[-0.03em]"
              >
                Create your first project
              </h2>
              <p className="leading-7 text-muted-foreground">
                Projects give your workspace a focused home for topology,
                resources, and future deployments.
              </p>
            </div>
            <Button size="lg" onClick={() => setIsCreateOpen(true)}>
              <PlusIcon data-icon="inline-start" />
              Create project
            </Button>
          </section>
        ) : (
          <section aria-labelledby="projects-title">
            <div className="workspace-projects-list-heading">
              <div>
                <h2
                  id="projects-title"
                  className="text-2xl font-semibold tracking-[-0.03em]"
                >
                  Projects
                </h2>
                <p className="mt-1 text-sm text-muted-foreground">
                  Choose a project to open its home.
                </p>
              </div>
              <span className="workspace-project-count">
                {projects.length}{" "}
                {projects.length === 1 ? "project" : "projects"}
              </span>
            </div>
            <div className="workspace-project-list">
              {projects.map((project) => (
                <Link
                  key={project.id}
                  to={`/workspace/${workspace.id}/project/${project.slug}`}
                  className="workspace-project-item"
                >
                  <span className="workspace-project-icon" aria-hidden="true">
                    <FolderKanbanIcon />
                  </span>
                  <span className="flex min-w-0 flex-1 flex-col gap-1">
                    <span className="truncate font-semibold text-foreground">
                      {project.name}
                    </span>
                    <span className="truncate text-sm text-muted-foreground">
                      /{project.slug}
                    </span>
                  </span>
                  <ArrowUpRightIcon
                    className="workspace-project-arrow"
                    aria-hidden="true"
                  />
                </Link>
              ))}
            </div>
          </section>
        )}
      </section>
      <ProjectCreateDialog
        key={isCreateOpen ? "project-dialog-open" : "project-dialog-closed"}
        workspaceId={currentWorkspaceId}
        open={isCreateOpen}
        onOpenChange={setIsCreateOpen}
        onCreated={handleProjectCreated}
      />
    </main>
  )
}

function WorkspaceFrame({ children }: { children: ReactNode }) {
  return (
    <main className="workspace-page">
      <div className="workspace-frame">
        <BrandMark />
        {children}
      </div>
    </main>
  )
}

function WorkspaceCreated({
  workspace,
  onContinue,
}: {
  workspace: Workspace
  onContinue: () => void
}) {
  return (
    <WorkspaceFrame>
      <div className="workspace-content workspace-success">
        <div className="workspace-ready-mark" aria-hidden="true">
          <CheckIcon />
        </div>
        <div className="flex flex-col gap-3 text-center">
          <h1 className="font-heading text-3xl font-semibold tracking-[-0.04em] text-foreground sm:text-4xl">
            Workspace created
          </h1>
          <p className="text-base leading-7 text-muted-foreground">
            {workspace.name} is ready for your ideas.
          </p>
        </div>
        <Button size="lg" className="h-11 w-full text-sm" onClick={onContinue}>
          Continue to workspace
        </Button>
      </div>
    </WorkspaceFrame>
  )
}
