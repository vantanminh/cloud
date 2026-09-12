export type User = {
  id: string
  fullName: string
  email: string
  emailVerified: boolean
}

export type Workspace = {
  id: string
  name: string
  slug: string
}

export type Project = {
  id: string
  name: string
  slug: string
}

export type AuthResponse = {
  user: User
  workspace: Workspace | null
}

export type ApiErrorPayload = {
  error?: {
    code?: string
    message?: string
    fields?: Record<string, string>
  }
}
