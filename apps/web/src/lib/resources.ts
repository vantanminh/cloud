import { apiRequest } from "@/lib/api"
import type { PostgresResource } from "@/lib/types"

export type CreatePostgresResourceInput = {
  name: string
}

function resourcesPath(workspaceSlug: string, projectSlug: string) {
  return `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}/resources`
}

export function listPostgresResources(
  workspaceSlug: string,
  projectSlug: string
) {
  return apiRequest<PostgresResource[]>(
    resourcesPath(workspaceSlug, projectSlug)
  )
}

export function createPostgresResource(
  workspaceSlug: string,
  projectSlug: string,
  input: CreatePostgresResourceInput
) {
  return apiRequest<PostgresResource>(
    resourcesPath(workspaceSlug, projectSlug),
    {
      method: "POST",
      body: { resourceType: "postgres", name: input.name },
    }
  )
}
