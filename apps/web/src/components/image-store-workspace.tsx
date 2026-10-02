import { useEffect, useState, type FormEvent } from "react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ApiError } from "@/lib/api"
import {
  createImageKey,
  deleteImageFolder,
  deleteImageObject,
  listImageKeys,
  listImageObjects,
  revokeImageKey,
  signImageObject,
  uploadImageObject,
} from "@/lib/resources"
import type {
  ImageApiKey,
  ImageKeyAccess,
  ImageObject,
  ImageStore,
} from "@/lib/types"

import "./resource-workspace.css"

type ImageStoreWorkspaceProps = {
  store: ImageStore
  workspaceId: string
  projectSlug: string
  onClose: () => void
  onChanged: () => void
}

export function ImageStoreWorkspace({
  store,
  workspaceId,
  projectSlug,
  onClose,
  onChanged,
}: ImageStoreWorkspaceProps) {
  const [keys, setKeys] = useState<ImageApiKey[]>([])
  const [objects, setObjects] = useState<ImageObject[]>([])
  const [folders, setFolders] = useState<string[]>([])
  const [error, setError] = useState<string | null>(null)
  const [keyName, setKeyName] = useState("Browser client")
  const [access, setAccess] = useState<ImageKeyAccess>("browser")
  const [createdSecret, setCreatedSecret] = useState<ImageApiKey | null>(null)
  const [folder, setFolder] = useState("posts")
  const [file, setFile] = useState<File | null>(null)
  const [signedUrl, setSignedUrl] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let active = true
    void Promise.all([
      listImageKeys(workspaceId, projectSlug, store.id),
      listImageObjects(workspaceId, projectSlug, store.id),
    ])
      .then(([nextKeys, nextObjects]) => {
        if (!active) {
          return
        }
        setKeys(nextKeys)
        setObjects(nextObjects.objects)
        setFolders(nextObjects.folders)
      })
      .catch((caught: unknown) => {
        if (active) {
          setError(messageFrom(caught))
        }
      })
    return () => {
      active = false
    }
  }, [projectSlug, store.id, workspaceId])

  async function handleCreateKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const key = await createImageKey(workspaceId, projectSlug, store.id, {
        name: keyName,
        access,
      })
      setCreatedSecret(key)
      setKeys((current) => [...current, { ...key, clientSecret: undefined }])
    } catch (caught: unknown) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  async function handleRevoke(keyId: string) {
    setBusy(true)
    setError(null)
    try {
      const revoked = await revokeImageKey(
        workspaceId,
        projectSlug,
        store.id,
        keyId
      )
      setKeys((current) =>
        current.map((key) => (key.id === revoked.id ? revoked : key))
      )
    } catch (caught: unknown) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  async function handleUpload(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!file) {
      setError("Choose an image to upload.")
      return
    }
    setBusy(true)
    setError(null)
    try {
      await uploadImageObject(workspaceId, projectSlug, store.id, file, folder)
      const next = await listImageObjects(workspaceId, projectSlug, store.id)
      setObjects(next.objects)
      setFolders(next.folders)
      setFile(null)
      onChanged()
    } catch (caught: unknown) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  async function handleDeleteFolder(target: string) {
    setBusy(true)
    setError(null)
    try {
      await deleteImageFolder(workspaceId, projectSlug, store.id, target)
      const next = await listImageObjects(workspaceId, projectSlug, store.id)
      setObjects(next.objects)
      setFolders(next.folders)
      onChanged()
    } catch (caught: unknown) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  async function handleDelete(imageId: string) {
    setBusy(true)
    setError(null)
    try {
      await deleteImageObject(workspaceId, projectSlug, store.id, imageId)
      setObjects((current) => current.filter((object) => object.id !== imageId))
      onChanged()
    } catch (caught: unknown) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  async function handleSign(object: ImageObject, visibility: "public" | "private") {
    setBusy(true)
    setError(null)
    try {
      const fullKey = keys.find(
        (key) => key.status === "active" && key.access === "full"
      )
      const signed = await signImageObject(
        workspaceId,
        projectSlug,
        store.id,
        object.id,
        {
          visibility,
          ...(visibility === "private"
            ? { expiresInSeconds: 3600, keyId: fullKey?.id }
            : {}),
          ...(store.compressionMode === "per_url"
            ? { width: 800, quality: store.quality ?? 80 }
            : {}),
        }
      )
      setSignedUrl(signed.url)
    } catch (caught: unknown) {
      setError(messageFrom(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div
      className="resource-workspace-overlay"
      role="dialog"
      aria-modal="true"
      aria-labelledby="image-store-title"
    >
      <div className="resource-workspace-sheet">
        <div className="resource-workspace-head">
          <div>
            <h2 id="image-store-title">{store.name}</h2>
            <p>
              {compressionSummary(store)}. Public URLs are cached for one year.
              Private URLs expire and use a 60 second cache.
            </p>
          </div>
          <button
            type="button"
            className="resource-workspace-close"
            aria-label="Close image store"
            onClick={onClose}
          >
            Close
          </button>
        </div>
        <div className="flex flex-col gap-6 p-6">
          {error && (
            <p role="alert" className="text-sm text-red-600">
              {error}
            </p>
          )}
          <section className="flex flex-col gap-3">
            <h3 className="text-base font-semibold">Client keys</h3>
            <p className="text-sm text-muted-foreground">
              Browser keys can upload, list, and sign public URLs from React or
              Next.js. Full keys can also delete images and sign private URLs.
              Revoking a key keeps already signed public URLs working.
            </p>
            <form className="flex flex-wrap items-end gap-3" onSubmit={handleCreateKey}>
              <label className="flex flex-col gap-1 text-sm">
                Key name
                <Input
                  aria-label="Image key name"
                  value={keyName}
                  onChange={(event) => setKeyName(event.target.value)}
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                Access
                <select
                  aria-label="Image key access"
                  className="border-input bg-background h-9 rounded-md border px-3 text-sm"
                  value={access}
                  onChange={(event) =>
                    setAccess(event.target.value as ImageKeyAccess)
                  }
                >
                  <option value="browser">Browser client</option>
                  <option value="full">Full server access</option>
                </select>
              </label>
              <Button type="submit" disabled={busy}>
                Create key
              </Button>
            </form>
            {createdSecret?.clientSecret && (
              <p className="text-sm">
                Client id <code>{createdSecret.clientId}</code>. Client secret{" "}
                <code>{createdSecret.clientSecret}</code>. Save the secret now.
                Knotree Cloud will not show it again.
              </p>
            )}
            <ul className="flex flex-col gap-2">
              {keys.map((key) => (
                <li key={key.id} className="flex flex-wrap items-center gap-3 text-sm">
                  <span>
                    {key.name} · {key.clientId} · {key.access} · {key.status}
                  </span>
                  {key.status === "active" && (
                    <Button
                      type="button"
                      variant="outline"
                      onClick={() => void handleRevoke(key.id)}
                      disabled={busy}
                    >
                      Revoke {key.name}
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          </section>
          <section className="flex flex-col gap-3">
            <h3 className="text-base font-semibold">Images</h3>
            <form className="flex flex-wrap items-end gap-3" onSubmit={handleUpload}>
              <label className="flex flex-col gap-1 text-sm">
                Folder
                <Input
                  aria-label="Image folder"
                  value={folder}
                  onChange={(event) => setFolder(event.target.value)}
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                File
                <input
                  aria-label="Image file"
                  type="file"
                  accept="image/png,image/jpeg,image/gif,image/webp"
                  onChange={(event) => setFile(event.target.files?.[0] ?? null)}
                />
              </label>
              <Button type="submit" disabled={busy}>
                Upload image
              </Button>
            </form>
            {folders.length > 0 && (
              <ul className="flex flex-col gap-2">
                {folders.map((name) => (
                  <li key={name} className="flex flex-wrap items-center gap-2 text-sm">
                    <span>{name}</span>
                    <Button
                      type="button"
                      variant="outline"
                      onClick={() => void handleDeleteFolder(name)}
                      disabled={busy}
                    >
                      Delete folder {name}
                    </Button>
                  </li>
                ))}
              </ul>
            )}
            <ul className="flex flex-col gap-2">
              {objects.map((object) => (
                <li key={object.id} className="flex flex-wrap items-center gap-2 text-sm">
                  <span>
                    {object.folder ? `${object.folder}/` : ""}
                    {object.fileName}
                  </span>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void handleSign(object, "public")}
                    disabled={busy}
                  >
                    Sign public URL
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void handleSign(object, "private")}
                    disabled={busy}
                  >
                    Sign private URL
                  </Button>
                  <Button
                    type="button"
                    variant="outline"
                    onClick={() => void handleDelete(object.id)}
                    disabled={busy}
                  >
                    Delete {object.fileName}
                  </Button>
                </li>
              ))}
            </ul>
            {signedUrl && (
              <label className="flex flex-col gap-1 text-sm">
                Signed URL
                <Input aria-label="Signed image URL" readOnly value={signedUrl} />
              </label>
            )}
          </section>
        </div>
      </div>
    </div>
  )
}

function compressionSummary(store: ImageStore) {
  if (store.compressionMode === "none") {
    return "Images keep their original format"
  }
  const size = [store.maxWidth && `${store.maxWidth}px wide`, store.maxHeight && `${store.maxHeight}px tall`]
    .filter(Boolean)
    .join(", ")
  const quality = store.quality ?? 80
  if (store.compressionMode === "fixed") {
    return `Every image is converted to WebP at quality ${quality}${size ? ` (${size})` : ""}`
  }
  return `Each URL can request its own WebP size, default quality ${quality}${size ? `, capped at ${size}` : ""}`
}

function messageFrom(error: unknown) {
  return error instanceof ApiError
    ? error.message
    : "The image store request failed."
}
