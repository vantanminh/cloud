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
  LoginRequest,
  RegisterRequest,
  Workspace,
} from "@/lib/auth-types"

type AuthStatus = "loading" | "ready"

type AuthContextValue = {
  status: AuthStatus
  session: AuthResponse | null
  bootstrapError: string | null
  signIn: (input: LoginRequest) => Promise<AuthResponse>
  signUp: (input: RegisterRequest) => Promise<AuthResponse>
  signOut: () => Promise<void>
  createWorkspace: (input: { name: string; slug: string }) => Promise<Workspace>
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

  const signIn = useCallback(async (input: LoginRequest) => {
    const nextSession = await apiRequest<AuthResponse>("/auth/login", {
      method: "POST",
      body: input,
    })
    setSession(nextSession)
    setBootstrapError(null)
    return nextSession
  }, [])

  const signUp = useCallback(async (input: RegisterRequest) => {
    const nextSession = await apiRequest<AuthResponse>("/auth/register", {
      method: "POST",
      body: input,
    })
    setSession(nextSession)
    setBootstrapError(null)
    return nextSession
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
    async (input: { name: string; slug: string }) => {
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
      signIn,
      signUp,
      signOut,
      createWorkspace,
    }),
    [
      bootstrapError,
      createWorkspace,
      session,
      signIn,
      signOut,
      signUp,
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
