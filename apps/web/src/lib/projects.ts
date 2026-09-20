import { apiRequest } from "@/lib/api"
import type { Project } from "@/lib/types"

export type CreateProjectInput = {
  name: string
  slug?: string
}

export function listProjects(workspaceId: string) {
  return apiRequest<Project[]>(
    `/workspaces/${encodeURIComponent(workspaceId)}/projects`
  )
}

export function createProject(
  workspaceId: string,
  input: CreateProjectInput
) {
  return apiRequest<Project>(
    `/workspaces/${encodeURIComponent(workspaceId)}/projects`,
    { method: "POST", body: input }
  )
}

export function getProject(workspaceId: string, projectSlug: string) {
  return apiRequest<Project>(
    `/workspaces/${encodeURIComponent(workspaceId)}/projects/${encodeURIComponent(projectSlug)}`
  )
}
