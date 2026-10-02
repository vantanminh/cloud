import { useState, type FormEvent } from "react"
import { Dialog } from "@base-ui/react/dialog"
import { ImageIcon } from "lucide-react"

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
import { createImageStore } from "@/lib/resources"
import type { ImageCompressionMode, ImageStore } from "@/lib/types"

type ImageStoreCreateDialogProps = {
  workspaceId: string
  projectSlug: string
  open: boolean
  onOpenChange: (open: boolean) => void
  onCreated: (store: ImageStore) => void
}

export function ImageStoreCreateDialog({
  workspaceId,
  projectSlug,
  open,
  onOpenChange,
  onCreated,
}: ImageStoreCreateDialogProps) {
  const [name, setName] = useState("Website images")
  const [compressionMode, setCompressionMode] =
    useState<ImageCompressionMode>("none")
  const [maxWidth, setMaxWidth] = useState("")
  const [maxHeight, setMaxHeight] = useState("")
  const [quality, setQuality] = useState("80")
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)
  const compresses = compressionMode !== "none"

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setSubmitError(null)
    setIsSubmitting(true)
    try {
      const store = await createImageStore(workspaceId, projectSlug, {
        name,
        compressionMode,
        ...(compresses
          ? {
              quality: Number(quality),
              ...(maxWidth ? { maxWidth: Number(maxWidth) } : {}),
              ...(maxHeight ? { maxHeight: Number(maxHeight) } : {}),
            }
          : {}),
      })
      onCreated(store)
    } catch (error: unknown) {
      if (error instanceof ApiError) {
        setErrors(error.fields)
        setSubmitError(error.message)
      } else {
        setSubmitError("The image store could not be created.")
      }
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop className="project-dialog-backdrop" />
        <Dialog.Popup className="project-dialog" aria-describedby={undefined}>
          <div className="project-dialog-heading">
            <div>
              <Dialog.Title className="project-dialog-title">
                Add image store
              </Dialog.Title>
              <Dialog.Description className="project-dialog-description">
                Store website images in folders and hand developers a client id
                and client secret.
              </Dialog.Description>
            </div>
            <button
              type="button"
              className="project-dialog-close"
              aria-label="Close create image store dialog"
              onClick={() => onOpenChange(false)}
            >
              ×
            </button>
          </div>
          <div className="project-dialog-note">
            <ImageIcon aria-hidden="true" />
            <span>
              Public URLs stay valid after a key is revoked and are cached for
              one year. Private URLs expire and use a 60 second cache. WebP
              compression is optional.
            </span>
          </div>
          {submitError && (
            <Alert variant="destructive">
              <AlertDescription>{submitError}</AlertDescription>
            </Alert>
          )}
          <form className="project-dialog-form" onSubmit={handleSubmit} noValidate>
            <FieldGroup>
              <Field data-invalid={Boolean(errors.name)}>
                <FieldLabel htmlFor="imageStoreName">Store name</FieldLabel>
                <Input
                  id="imageStoreName"
                  name="imageStoreName"
                  value={name}
                  autoComplete="off"
                  onChange={(event) => setName(event.target.value)}
                />
                <FieldError>{errors.name}</FieldError>
              </Field>
              <Field data-invalid={Boolean(errors.compressionMode)}>
                <FieldLabel htmlFor="imageCompressionMode">
                  Compression
                </FieldLabel>
                <select
                  id="imageCompressionMode"
                  className="border-input bg-background h-9 w-full rounded-md border px-3 text-sm"
                  value={compressionMode}
                  onChange={(event) =>
                    setCompressionMode(event.target.value as ImageCompressionMode)
                  }
                >
                  <option value="none">Keep the original format</option>
                  <option value="fixed">WebP with one saved size</option>
                  <option value="per_url">WebP with a size on each URL</option>
                </select>
                <FieldDescription>
                  Fixed compression applies the same WebP settings to every
                  image. Per-URL compression lets each signed URL choose its
                  own size.
                </FieldDescription>
                <FieldError>{errors.compressionMode}</FieldError>
              </Field>
              {compresses && (
                <>
                  <Field data-invalid={Boolean(errors.quality)}>
                    <FieldLabel htmlFor="imageQuality">WebP quality</FieldLabel>
                    <Input
                      id="imageQuality"
                      inputMode="numeric"
                      value={quality}
                      onChange={(event) => setQuality(event.target.value)}
                    />
                    <FieldError>{errors.quality}</FieldError>
                  </Field>
                  <Field data-invalid={Boolean(errors.maxWidth)}>
                    <FieldLabel htmlFor="imageMaxWidth">Max width</FieldLabel>
                    <Input
                      id="imageMaxWidth"
                      inputMode="numeric"
                      value={maxWidth}
                      placeholder="Original width"
                      onChange={(event) => setMaxWidth(event.target.value)}
                    />
                    <FieldError>{errors.maxWidth}</FieldError>
                  </Field>
                  <Field data-invalid={Boolean(errors.maxHeight)}>
                    <FieldLabel htmlFor="imageMaxHeight">Max height</FieldLabel>
                    <Input
                      id="imageMaxHeight"
                      inputMode="numeric"
                      value={maxHeight}
                      placeholder="Original height"
                      onChange={(event) => setMaxHeight(event.target.value)}
                    />
                    <FieldError>{errors.maxHeight}</FieldError>
                  </Field>
                </>
              )}
            </FieldGroup>
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? <Spinner /> : "Create image store"}
            </Button>
          </form>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  )
}
