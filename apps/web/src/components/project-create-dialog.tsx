import { useState, type FormEvent } from "react"
import { Dialog } from "@base-ui/react/dialog"

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
import { createProject } from "@/lib/projects"
import { slugifyProjectName } from "@/lib/slug"
import type { Project } from "@/lib/types"

type ProjectCreateDialogProps = {
  workspaceId: string
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated: (project: Project) => void
}

export function ProjectCreateDialog({
  workspaceId,
  open,
  onOpenChange,
  onCreated,
}: ProjectCreateDialogProps) {
  const [name, setName] = useState("")
  const [slug, setSlug] = useState("")
  const [isCustomSlug, setIsCustomSlug] = useState(false)
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)

  function handleNameChange(value: string) {
    setName(value)
    if (!isCustomSlug) {
      setSlug(slugifyProjectName(value))
    }
    setSubmitError(null)
  }

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextErrors: Record<string, string> = {}
    if (name.trim().length === 0 || name.trim().length > 80) {
      nextErrors.name = "Enter a project name between 1 and 80 characters."
    }
    if (
      isCustomSlug &&
      (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(slug) || slug.length > 48)
    ) {
      nextErrors.slug = "Use a lowercase project URL slug."
    }
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }

    setIsSubmitting(true)
    try {
      const project = await createProject(workspaceId, {
        name: name.trim(),
        ...(isCustomSlug ? { slug } : {}),
      })
      onCreated(project)
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
        if (!isSubmitting) {
          onOpenChange(nextOpen)
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Backdrop className="project-dialog-backdrop" />
        <Dialog.Popup className="project-dialog-popup">
          <div className="project-dialog-header">
            <div>
              <Dialog.Title className="project-dialog-title">
                Create a project
              </Dialog.Title>
              <Dialog.Description className="project-dialog-description">
                Give your project a clear home for its infrastructure.
              </Dialog.Description>
            </div>
            <button
              type="button"
              className="project-dialog-close"
              aria-label="Close create project dialog"
              onClick={() => onOpenChange(false)}
            >
              ×
            </button>
          </div>

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
                <FieldLabel htmlFor="projectName">Project name</FieldLabel>
                <Input
                  id="projectName"
                  name="projectName"
                  value={name}
                  placeholder="Knotree Study"
                  autoComplete="off"
                  aria-invalid={Boolean(errors.name)}
                  onChange={(event) => handleNameChange(event.target.value)}
                />
                {errors.name && <FieldError>{errors.name}</FieldError>}
              </Field>
            </FieldGroup>
            <div className="project-dialog-url">
              <div>
                <p className="text-sm font-medium text-foreground">
                  Project URL
                </p>
                <p className="mt-1 text-sm text-muted-foreground">
                  {slug ? `/${slug}` : "Generated from the project name"}
                </p>
              </div>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                aria-expanded={isCustomSlug}
                aria-controls={isCustomSlug ? "projectSlugField" : undefined}
                onClick={() => {
                  if (isCustomSlug) {
                    setSlug(slugifyProjectName(name))
                  }
                  setIsCustomSlug((current) => !current)
                  setErrors((current) => ({ ...current, slug: "" }))
                  setSubmitError(null)
                }}
              >
                {isCustomSlug ? "Use suggested URL" : "Customize URL"}
              </Button>
            </div>
            {isCustomSlug && (
              <Field id="projectSlugField" data-invalid={Boolean(errors.slug)}>
                <FieldLabel htmlFor="projectSlug">Custom project URL</FieldLabel>
                <Input
                  id="projectSlug"
                  name="projectSlug"
                  value={slug}
                  placeholder="knotree-study"
                  autoComplete="off"
                  spellCheck={false}
                  aria-invalid={Boolean(errors.slug)}
                  onChange={(event) => {
                    setSlug(event.target.value.toLowerCase())
                    setErrors((current) => ({ ...current, slug: "" }))
                    setSubmitError(null)
                  }}
                />
                {errors.slug && <FieldError>{errors.slug}</FieldError>}
              </Field>
            )}

            <div className="project-dialog-actions">
              <Button
                type="button"
                variant="ghost"
                disabled={isSubmitting}
                onClick={() => onOpenChange(false)}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting && <Spinner data-icon="inline-start" />}
                Create project
              </Button>
            </div>
          </form>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
