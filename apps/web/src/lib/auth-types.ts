import type { AuthResponse, Workspace } from "@/lib/types"

export type LoginRequest = {
  email: string
  password: string
}

export type RegisterRequest = LoginRequest & {
  fullName: string
}

export type CreateWorkspaceResponse = Workspace

export type { AuthResponse, Workspace }
