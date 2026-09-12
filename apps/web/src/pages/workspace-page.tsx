import { useState, type FormEvent, type ReactNode } from "react"
import { CheckIcon, LogOutIcon } from "lucide-react"
import { useNavigate, useParams } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { BrandMark } from "@/components/brand-mark"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupInput,
  InputGroupText,
} from "@/components/ui/input-group"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import { slugifyWorkspaceName } from "@/lib/slug"
import type { Workspace } from "@/lib/types"

export function NewWorkspacePage() {
  const navigate = useNavigate()
  const { createWorkspace } = useAuth()
  const [name, setName] = useState("")
  const [slug, setSlug] = useState("")
  const [slugTouched, setSlugTouched] = useState(false)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const [createdWorkspace, setCreatedWorkspace] = useState<Workspace | null>(null)

  function handleNameChange(value: string) {
    setName(value)
    if (!slugTouched) {
      setSlug(slugifyWorkspaceName(value))
    }
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextErrors: Record<string, string> = {}
    if (name.trim().length === 0 || name.trim().length > 80) {
      nextErrors.name = "Enter a name between 1 and 80 characters."
    }
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug) || slug.length > 48) {
      nextErrors.slug = "Use a lowercase workspace URL slug."
    }
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }

    setIsSubmitting(true)
    try {
      const workspace = await createWorkspace({ name: name.trim(), slug })
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
        onContinue={() => navigate(`/workspace/${createdWorkspace.slug}`)}
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

        <form className="flex flex-col gap-6" onSubmit={handleSubmit} noValidate>
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
                onChange={(event) => handleNameChange(event.target.value)}
              />
              {errors.name && <FieldError>{errors.name}</FieldError>}
            </Field>
            <Field data-invalid={Boolean(errors.slug)}>
              <FieldLabel htmlFor="workspaceSlug">Workspace URL</FieldLabel>
              <InputGroup>
                <InputGroupAddon align="inline-start">
                  <InputGroupText className="max-w-56 truncate text-xs sm:max-w-none">
                    cloud.knotree.com/workspace/
                  </InputGroupText>
                </InputGroupAddon>
                <InputGroupInput
                  id="workspaceSlug"
                  name="workspaceSlug"
                  value={slug}
                  autoComplete="off"
                  spellCheck={false}
                  aria-invalid={Boolean(errors.slug)}
                  onChange={(event) => {
                    setSlugTouched(true)
                    setSlug(event.target.value.toLowerCase())
                  }}
                />
              </InputGroup>
              {errors.slug && <FieldError>{errors.slug}</FieldError>}
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
          You can update these details later.
        </p>
      </div>
    </WorkspaceFrame>
  )
}

export function WorkspacePage() {
  const { session, signOut } = useAuth()
  const { slug } = useParams()
  const navigate = useNavigate()
  const workspace = session?.workspace

  async function handleSignOut() {
    try {
      await signOut()
    } finally {
      navigate("/login", { replace: true })
    }
  }

  if (!session || !workspace || workspace.slug !== slug) {
    return null
  }

  return (
    <main className="min-h-svh bg-background">
      <header className="border-b border-border/80 bg-background">
        <div className="mx-auto flex max-w-6xl items-center justify-between px-5 py-5 sm:px-8">
          <BrandMark compact />
          <div className="flex items-center gap-4">
            <span className="hidden text-sm text-muted-foreground sm:inline">
              {session.user.email}
            </span>
            <Button variant="ghost" size="sm" onClick={handleSignOut}>
              <LogOutIcon data-icon="inline-start" />
              Sign out
            </Button>
          </div>
        </div>
      </header>
      <section className="mx-auto flex max-w-6xl flex-col gap-8 px-5 py-16 sm:px-8 sm:py-24">
        <div className="workspace-ready-mark" aria-hidden="true">
          <CheckIcon />
        </div>
        <div className="flex max-w-xl flex-col gap-3">
          <p className="text-sm font-medium uppercase tracking-[0.18em] text-primary">
            Workspace ready
          </p>
          <h1 className="font-heading text-4xl font-semibold tracking-[-0.04em] text-foreground sm:text-5xl">
            Welcome to {workspace.name}
          </h1>
          <p className="text-lg leading-8 text-muted-foreground">
            Your first Knotree Cloud workspace is ready at cloud.knotree.com/workspace/{workspace.slug}.
          </p>
        </div>
      </section>
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
