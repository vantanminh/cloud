import { useEffect, useState, type FormEvent, type ReactNode } from "react"
import {
  ArrowUpRightIcon,
  CheckIcon,
  FolderKanbanIcon,
  PlugIcon,
  PlusIcon,
  SearchIcon,
} from "lucide-react"
import { Link, Navigate, useNavigate, useParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import {
  AppShell,
  BreadcrumbSeparator,
  SidebarNavItem,
} from "@/components/app-shell"
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
import { initials } from "@/lib/initials"
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
        <div className="workspace-step">
          <div className="workspace-stepper" aria-label="Step 1 of 2">
            <span className="workspace-stepper-active" />
            <span />
          </div>
          <span aria-hidden="true">Step 1 of 2</span>
        </div>
        <div className="flex flex-col gap-2">
          <h1 className="text-[1.375rem] font-semibold tracking-[-0.025em] text-foreground">
            Create your first workspace
          </h1>
          <p className="text-sm leading-6 text-muted-foreground">
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
            className="h-9 w-full text-sm"
          >
            {isSubmitting && <Spinner data-icon="inline-start" />}
            Create workspace
          </Button>
        </form>

        <p className="text-center text-[0.8125rem] text-muted-foreground">
          Projects and services can be added any time.
        </p>
      </div>
    </WorkspaceFrame>
  )
}

export function WorkspacePage() {
  const { session } = useAuth()
  const { workspaceId: routeWorkspaceId } = useParams()
  const navigate = useNavigate()
  const workspace = session?.workspace
  const [projects, setProjects] = useState<Project[] | null>(null)
  const [projectsError, setProjectsError] = useState<string | null>(null)
  const [isCreateOpen, setIsCreateOpen] = useState(false)
  const [search, setSearch] = useState("")
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

  if (!session || !workspace || workspace.id !== routeWorkspaceId) {
    return null
  }

  const currentWorkspaceId = workspace.id
  const normalizedSearch = search.trim().toLowerCase()
  const visibleProjects = (projects ?? []).filter(
    (project) =>
      !normalizedSearch ||
      `${project.name} ${project.slug}`.toLowerCase().includes(normalizedSearch)
  )

  function handleProjectCreated(project: Project) {
    setProjects((currentProjects) =>
      currentProjects ? [...currentProjects, project] : [project]
    )
    setIsCreateOpen(false)
    navigate(`/workspace/${currentWorkspaceId}/project/${project.slug}`)
  }

  return (
    <AppShell
      workspace={workspace}
      user={session.user}
      nav={
        <SidebarNavItem
          label="Projects"
          icon={<FolderKanbanIcon />}
          active
          to={`/workspace/${workspace.id}`}
        />
      }
      footerNav={
        <SidebarNavItem
          label="Integrations"
          icon={<PlugIcon />}
          to="/settings/integrations"
        />
      }
      breadcrumbs={
        <>
          <span className="app-breadcrumb-current">{workspace.name}</span>
          <BreadcrumbSeparator />
          <span className="app-breadcrumb-link">Projects</span>
        </>
      }
      actions={
        <Button size="sm" onClick={() => setIsCreateOpen(true)}>
          <PlusIcon data-icon="inline-start" />
          New project
        </Button>
      }
    >
      <div className="app-content">
        <div className="app-page">
          <div className="page-header">
            <div>
              <span className="page-eyebrow">Workspace</span>
              <h1>Welcome to {workspace.name}</h1>
              <p>
                Each project groups its services, databases and private network.
              </p>
            </div>
          </div>

          {projectsError && (
            <Alert variant="destructive" className="mb-6">
              <AlertDescription>{projectsError}</AlertDescription>
            </Alert>
          )}

          {projects === null ? (
            <div className="loading-row" role="status">
              <Spinner />
              <span>Loading projects</span>
            </div>
          ) : projects.length === 0 ? (
            <section
              className="empty-panel"
              aria-labelledby="empty-projects-title"
            >
              <span className="empty-icon" aria-hidden="true">
                <FolderKanbanIcon />
              </span>
              <div>
                <h2 id="empty-projects-title">Create your first project</h2>
                <p>
                  Projects give your workspace a focused home for topology,
                  resources, and deployments.
                </p>
              </div>
              <Button onClick={() => setIsCreateOpen(true)}>
                <PlusIcon data-icon="inline-start" />
                Create project
              </Button>
              <ol className="empty-steps" aria-label="How projects work">
                <li>
                  <span>01</span>Create a project
                </li>
                <li>
                  <span>02</span>Add services and data
                </li>
                <li>
                  <span>03</span>Deploy and observe
                </li>
              </ol>
            </section>
          ) : (
            <section aria-labelledby="projects-title">
              <div className="projects-toolbar">
                <div className="flex items-baseline gap-2.5">
                  <h2 id="projects-title">Projects</h2>
                  <span>{projects.length}</span>
                </div>
                <label className="search-field">
                  <SearchIcon aria-hidden="true" />
                  <span className="sr-only">Search projects</span>
                  <input
                    type="search"
                    value={search}
                    placeholder="Search projects"
                    onChange={(event) => setSearch(event.target.value)}
                  />
                </label>
              </div>
              <div className="workspace-project-list">
                {visibleProjects.map((project) => (
                  <Link
                    key={project.id}
                    to={`/workspace/${workspace.id}/project/${project.slug}`}
                    className="workspace-project-item"
                  >
                    <span className="workspace-project-item-head">
                      <span className="project-initial" aria-hidden="true">
                        {initials(project.name)}
                      </span>
                      <ArrowUpRightIcon
                        className="workspace-project-arrow"
                        aria-hidden="true"
                      />
                    </span>
                    <span className="workspace-project-item-body">
                      <strong>{project.name}</strong>
                      <span>/{project.slug}</span>
                    </span>
                  </Link>
                ))}
                {!normalizedSearch && (
                  <button
                    type="button"
                    className="workspace-project-item workspace-project-new"
                    onClick={() => setIsCreateOpen(true)}
                  >
                    <PlusIcon aria-hidden="true" />
                    New project
                  </button>
                )}
              </div>
              {visibleProjects.length === 0 && (
                <p className="loading-row">
                  No projects match &ldquo;{search.trim()}&rdquo;.
                </p>
              )}
            </section>
          )}
        </div>
      </div>
      <ProjectCreateDialog
        key={isCreateOpen ? "project-dialog-open" : "project-dialog-closed"}
        workspaceId={currentWorkspaceId}
        open={isCreateOpen}
        onOpenChange={setIsCreateOpen}
        onCreated={handleProjectCreated}
      />
    </AppShell>
  )
}

function WorkspaceFrame({ children }: { children: ReactNode }) {
  return (
    <main className="workspace-page">
      <header>
        <BrandMark compact />
      </header>
      <div className="workspace-frame">{children}</div>
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
        <div className="flex flex-col gap-2 text-center">
          <h1 className="text-[1.375rem] font-semibold tracking-[-0.025em] text-foreground">
            Workspace created
          </h1>
          <p className="text-sm leading-6 text-muted-foreground">
            {workspace.name} is ready for your ideas.
          </p>
        </div>
        <Button size="lg" className="h-9 w-full text-sm" onClick={onContinue}>
          Continue to workspace
        </Button>
      </div>
    </WorkspaceFrame>
  )
}
