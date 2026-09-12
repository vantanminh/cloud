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
import { AuthPage } from "@/pages/auth-page"
import { NewWorkspacePage, WorkspacePage } from "@/pages/workspace-page"

export default function App() {
  return (
    <Routes>
      <Route path="/" element={<SessionDestination />} />
      <Route element={<PublicOnlyRoute />}>
        <Route path="/login" element={<AuthPage mode="login" />} />
        <Route path="/register" element={<AuthPage mode="register" />} />
      </Route>
      <Route element={<ProtectedRoute />}>
        <Route path="/new/workspace" element={<WorkspaceCreationRoute />} />
        <Route path="/workspace/:slug" element={<WorkspaceRoute />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
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

function WorkspaceRoute() {
  const { session } = useAuth()
  const { slug } = useParams()
  if (!session?.workspace) {
    return <Navigate to="/new/workspace" replace />
  }
  if (session.workspace.slug !== slug) {
    return <Navigate to={destinationFor(session.workspace)} replace />
  }
  return <WorkspacePage />
}

function destinationFor(workspace: { slug: string } | null) {
  return workspace ? `/workspace/${workspace.slug}` : "/new/workspace"
}
