import {
  Navigate,
  Outlet,
  Route,
  Routes,
  useLocation,
  useParams,
} from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { LoadingScreen } from "@/components/loading-screen"
import { ThemeProvider } from "@/components/theme-provider"
import { AuthPage } from "@/pages/auth-page"
import { GitHubIntegrationPage } from "@/pages/github-integration-page"
import { ProjectHomePage } from "@/pages/project-home-page"
import { NewWorkspacePage, WorkspacePage } from "@/pages/workspace-page"

export default function App() {
  return (
    <ThemeProvider>
      <Routes>
        <Route path="/" element={<SessionDestination />} />
        <Route element={<PublicOnlyRoute />}>
          <Route path="/login" element={<AuthPage mode="login" />} />
          <Route path="/register" element={<AuthPage mode="register" />} />
        </Route>
        <Route element={<ProtectedRoute />}>
          <Route path="/new/workspace" element={<WorkspaceCreationRoute />} />
          <Route path="/settings" element={<SettingsRoute />} />
          <Route
            path="/settings/integrations"
            element={<GitHubIntegrationPage />}
          />
          <Route
            path="/workspace/:workspaceId/project/:projectSlug"
            element={<ProjectRoute />}
          />
          <Route path="/workspace/:workspaceId" element={<WorkspaceRoute />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </ThemeProvider>
  )
}

function SessionDestination() {
  const { status, session } = useAuth()
  if (status === "loading") {
    return <LoadingScreen />
  }
  return (
    <Navigate
      to={session ? destinationFor(session.workspace) : "/login"}
      replace
    />
  )
}

function PublicOnlyRoute() {
  const { status, session } = useAuth()
  if (status === "loading") {
    return <LoadingScreen />
  }
  if (session) {
    return <Navigate to={destinationFor(session.workspace)} replace />
  }
  return <Outlet />
}

function ProtectedRoute() {
  const { status, session } = useAuth()
  const location = useLocation()
  if (status === "loading") {
    return <LoadingScreen />
  }
  if (!session) {
    return <Navigate to="/login" state={{ from: location.pathname }} replace />
  }
  return <Outlet />
}

function WorkspaceCreationRoute() {
  return <NewWorkspacePage />
}

function SettingsRoute() {
  return <Navigate to="/settings/integrations" replace />
}

function WorkspaceRoute() {
  const { session } = useAuth()
  const { workspaceId } = useParams()
  if (!session?.workspace) {
    return <Navigate to="/new/workspace" replace />
  }
  if (session.workspace.id !== workspaceId) {
    return <Navigate to={destinationFor(session.workspace)} replace />
  }
  return <WorkspacePage />
}

function ProjectRoute() {
  const { session } = useAuth()
  const { workspaceId, projectSlug } = useParams()
  if (!session?.workspace) {
    return <Navigate to="/new/workspace" replace />
  }
  if (session.workspace.id !== workspaceId) {
    if (projectSlug) {
      return (
        <Navigate
          to={`/workspace/${session.workspace.id}/project/${projectSlug}`}
          replace
        />
      )
    }
    return <Navigate to={destinationFor(session.workspace)} replace />
  }
  return <ProjectHomePage key={`${workspaceId}/${projectSlug}`} />
}

function destinationFor(workspace: { id: string } | null) {
  return workspace ? `/workspace/${workspace.id}` : "/new/workspace"
}
