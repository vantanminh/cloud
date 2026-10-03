/* eslint-disable react-refresh/only-export-components */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react"

import {
  apiRequest,
  ApiError,
  getCsrfToken,
  resetCsrfToken,
} from "@/lib/api"
import type {
  AuthResponse,
  CreateWorkspaceResponse,
  Workspace,
} from "@/lib/auth-types"

type AuthStatus = "loading" | "ready"

type AuthContextValue = {
  status: AuthStatus
  session: AuthResponse | null
  bootstrapError: string | null
  signOut: () => Promise<void>
  createWorkspace: (input: { name: string }) => Promise<Workspace>
}

const AuthContext = createContext<AuthContextValue | null>(null)

export function AuthProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<AuthStatus>("loading")
  const [session, setSession] = useState<AuthResponse | null>(null)
  const [bootstrapError, setBootstrapError] = useState<string | null>(null)

  useEffect(() => {
    let active = true

    async function bootstrap() {
      const sessionRequest = apiRequest<AuthResponse>("/auth/me").catch(
        (error: unknown) => {
          if (error instanceof ApiError && error.status === 401) {
            return null
          }
          throw error
        }
      )

      try {
        const [, currentSession] = await Promise.all([
          getCsrfToken(),
          sessionRequest,
        ])
        if (!active) {
          return
        }
        setSession(currentSession)
        setStatus("ready")
      } catch (error) {
        if (!active) {
          return
        }
        setStatus("ready")
        setBootstrapError(
          error instanceof Error
            ? error.message
            : "The API is currently unavailable."
        )
      }
    }

    void bootstrap()
    return () => {
      active = false
    }
  }, [])

  const signOut = useCallback(async () => {
    try {
      await apiRequest<void>("/auth/logout", { method: "POST" })
    } finally {
      setSession(null)
      resetCsrfToken()
    }
  }, [])

  const createWorkspace = useCallback(
    async (input: { name: string }) => {
      const workspace = await apiRequest<CreateWorkspaceResponse>(
        "/workspaces",
        { method: "POST", body: input }
      )
      setSession((currentSession) =>
        currentSession ? { ...currentSession, workspace } : currentSession
      )
      return workspace
    },
    []
  )

  const value = useMemo(
    () => ({
      status,
      session,
      bootstrapError,
      signOut,
      createWorkspace,
    }),
    [
      bootstrapError,
      createWorkspace,
      session,
      signOut,
      status,
    ]
  )

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
}

export function useAuth() {
  const context = useContext(AuthContext)
  if (!context) {
    throw new Error("useAuth must be used within AuthProvider")
  }
  return context
}
