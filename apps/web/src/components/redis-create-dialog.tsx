import { useState, type FormEvent } from "react"
import { Dialog } from "@base-ui/react/dialog"
import { DatabaseIcon } from "lucide-react"

import { Alert, AlertDescription } from "@/components/ui/alert"
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
import { createRedisResource } from "@/lib/resources"
import type { RedisResource } from "@/lib/types"

type RedisCreateDialogProps = {
  workspaceSlug: string
  projectSlug: string
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated: (resource: RedisResource) => void
}

export function RedisCreateDialog({
  workspaceSlug,
  projectSlug,
  open,
  onOpenChange,
  onCreated,
}: RedisCreateDialogProps) {
  const [name, setName] = useState("Redis")
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextErrors: Record<string, string> = {}
    if (name.trim().length === 0 || name.trim().length > 80) {
      nextErrors.name = "Enter a Redis name between 1 and 80 characters."
    }
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }

    setIsSubmitting(true)
    try {
      const resource = await createRedisResource(workspaceSlug, projectSlug, {
        name: name.trim(),
      })
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
        if (!isSubmitting) {
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
                Create Redis
              </Dialog.Title>
              <Dialog.Description className="project-dialog-description">
                Knotree will provision a capped Redis instance on this
                project&apos;s private network.
              </Dialog.Description>
            </div>
            <button
              type="button"
              className="project-dialog-close"
              aria-label="Close create Redis dialog"
              onClick={() => onOpenChange(false)}
            >
              ×
            </button>
          </div>

          <div className="project-dialog-note">
            <DatabaseIcon aria-hidden="true" />
            <span>
              Redis is capped at 1 vCPU, 1 GiB RAM, and 10 GiB storage and is
              reachable as hostname redis.
            </span>
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
                <FieldLabel htmlFor="redisName">Redis name</FieldLabel>
                <Input
                  id="redisName"
                  name="redisName"
                  value={name}
                  autoComplete="off"
                  aria-invalid={Boolean(errors.name)}
                  onChange={(event) => setName(event.target.value)}
                />
                <FieldDescription>
                  This is the display name shown on the topology canvas.
                </FieldDescription>
                {errors.name && <FieldError>{errors.name}</FieldError>}
              </Field>
            </FieldGroup>

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
                Create Redis
              </Button>
            </div>
          </form>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
