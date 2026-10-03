import { useEffect, useState } from "react"
import { ContainerIcon, SearchIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import {
  listKnotreeRegistryRepositories,
  listKnotreeRegistryTags,
} from "@/lib/resources"
import type {
  KnotreeRegistryAccountStatus,
  KnotreeRegistryRepository,
  KnotreeRegistryTag,
} from "@/lib/types"

type KnotreeRegistryImagePickerProps = {
  account: KnotreeRegistryAccountStatus | null
  accountLoading: boolean
  selectedRepository: string
  disabled?: boolean
  onConnect: () => void
  onSelect: (repository: string, tag: string) => void
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof ApiError ? error.message : fallback
}

/**
 * Lists only the signed-in user's own Registry namespace (the API uses that
 * user's account credential) and picks a repository and tag to deploy.
 */
export function KnotreeRegistryImagePicker({
  account,
  accountLoading,
  selectedRepository,
  disabled,
  onConnect,
  onSelect,
}: KnotreeRegistryImagePickerProps) {
  const usable = Boolean(account?.connected && !account.expired)
  const [repositories, setRepositories] = useState<
    KnotreeRegistryRepository[] | null
  >(null)
  const [query, setQuery] = useState("")
  const [tags, setTags] = useState<KnotreeRegistryTag[] | null>(null)
  const [tagsFor, setTagsFor] = useState("")
  const [selectedTag, setSelectedTag] = useState("")
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!usable) return undefined
    let active = true
    void listKnotreeRegistryRepositories()
      .then((result) => {
        if (active) setRepositories(result.repositories)
      })
      .catch((reason: unknown) => {
        if (active) {
          setRepositories([])
          setError(
            errorMessage(reason, "Your Registry images could not be loaded.")
          )
        }
      })
    return () => {
      active = false
    }
  }, [usable])

  async function chooseRepository(repository: KnotreeRegistryRepository) {
    setError(null)
    setTags(null)
    setTagsFor(repository.name)
    try {
      const result = await listKnotreeRegistryTags(repository.name)
      setTags(result.tags)
      const initial =
        result.tags.find((tag) => tag.tag === repository.latestTag)?.tag ??
        result.tags.find((tag) => tag.tag === "latest")?.tag ??
        result.tags[0]?.tag ??
        ""
      setSelectedTag(initial)
      if (initial) onSelect(repository.name, initial)
    } catch (reason) {
      setTags([])
      setError(errorMessage(reason, "Tags could not be loaded."))
    }
  }

  if (accountLoading) {
    return (
      <div className="project-registry-picker" role="status">
        <Spinner /> <span>Checking your Knotree Registry connection…</span>
      </div>
    )
  }

  if (!usable) {
    return (
      <div className="project-registry-picker">
        <p className="project-dialog-description">
          {account?.expired
            ? "Your Knotree Registry connection expired. Reconnect to import images."
            : "Connect your Knotree Registry account once to pick your images here and redeploy on every push."}
        </p>
        <Button
          type="button"
          variant="outline"
          disabled={disabled || account?.consentReady === false}
          onClick={onConnect}
        >
          <ContainerIcon data-icon="inline-start" />
          {account?.expired
            ? "Reconnect Knotree Registry"
            : "Connect Knotree Registry"}
        </Button>
      </div>
    )
  }

  const visible = (repositories ?? []).filter((repository) =>
    repository.name.toLowerCase().includes(query.trim().toLowerCase())
  )

  return (
    <div className="project-registry-picker">
      <Field>
        <FieldLabel htmlFor="knotreeRegistrySearch">
          Import from {account?.namespace}
        </FieldLabel>
        <div className="project-registry-search">
          <SearchIcon aria-hidden="true" />
          <Input
            id="knotreeRegistrySearch"
            value={query}
            placeholder="Search your images"
            autoComplete="off"
            onChange={(event) => setQuery(event.target.value)}
          />
        </div>
        {repositories === null ? (
          <div role="status">
            <Spinner /> <span>Loading your images…</span>
          </div>
        ) : visible.length === 0 ? (
          <FieldDescription>
            {repositories.length === 0
              ? `No images yet. Push one with docker push registry.knotree.com/${account?.namespace}/app:latest.`
              : "No images match your search."}
          </FieldDescription>
        ) : (
          <ul
            className="project-registry-repositories"
            aria-label="Your Registry images"
          >
            {visible.map((repository) => (
              <li key={repository.name}>
                <button
                  type="button"
                  disabled={disabled}
                  aria-pressed={selectedRepository === repository.name}
                  onClick={() => void chooseRepository(repository)}
                >
                  <span>{repository.name}</span>
                  <small>
                    {repository.tagCount} tag
                    {repository.tagCount === 1 ? "" : "s"}
                    {repository.latestTag ? ` · ${repository.latestTag}` : ""}
                  </small>
                </button>
              </li>
            ))}
          </ul>
        )}
      </Field>
      {tagsFor && (
        <Field>
          <FieldLabel htmlFor="knotreeRegistryTag">Tag</FieldLabel>
          {tags === null ? (
            <div role="status">
              <Spinner /> <span>Loading tags…</span>
            </div>
          ) : (
            <select
              id="knotreeRegistryTag"
              className="project-dialog-select"
              value={selectedTag}
              disabled={disabled || tags.length === 0}
              onChange={(event) => {
                setSelectedTag(event.target.value)
                onSelect(tagsFor, event.target.value)
              }}
            >
              {tags.length === 0 && <option value="">No tags</option>}
              {tags.map((tag) => (
                <option key={tag.tag} value={tag.tag}>
                  {tag.tag} · {tag.digest.slice(7, 19)}
                </option>
              ))}
            </select>
          )}
          <FieldDescription>
            With automatic deploys on, every push to this tag rolls out the new
            digest.
          </FieldDescription>
        </Field>
      )}
      {error && (
        <p className="project-dialog-description" role="alert">
          {error}
        </p>
      )}
    </div>
  )
}
