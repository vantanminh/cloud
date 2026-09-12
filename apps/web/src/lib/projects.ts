import { apiRequest } from "@/lib/api"
import type { Project } from "@/lib/types"

export type CreateProjectInput = {
  name: string
  slug: string
}

export function listProjects(workspaceSlug: string) {
  return apiRequest<Project[]>(
    `/workspaces/${encodeURIComponent(workspaceSlug)}/projects`
  )
}

export function createProject(
  workspaceSlug: string,
  input: CreateProjectInput
) {
  return apiRequest<Project>(
    `/workspaces/${encodeURIComponent(workspaceSlug)}/projects`,
    { method: "POST", body: input }
  )
}

export function getProject(workspaceSlug: string, projectSlug: string) {
  return apiRequest<Project>(
    `/workspaces/${encodeURIComponent(workspaceSlug)}/projects/${encodeURIComponent(projectSlug)}`
  )
}
