import { useEffect, useRef, useState, type ReactNode } from "react"
import { ChevronsUpDownIcon, LogOutIcon, MoonIcon, SunIcon } from "lucide-react"
import { cn } from "cn"
import { Link, useNavigate } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { initials } from "@/lib/initials"
import { EnsureThemeProvider, useTheme } from "@/components/theme-provider"
import type { User, Workspace } from "@/lib/types"

type AppShellProps = {
  workspace: Workspace
  user: User
  /** Primary navigation for the current scope (workspace or project). */
  nav: ReactNode
  /** Secondary navigation pinned to the bottom of the sidebar. */
  footerNav?: ReactNode
  /** Context shown above the primary navigation (e.g. the open project). */
  context?: ReactNode
  breadcrumbs: ReactNode
  actions?: ReactNode
  className?: string
  children: ReactNode
}

/**
 * Product chrome shared by every signed-in screen: a quiet sidebar on the
 * canvas, and an inset panel holding the header and the page content. Below
 * 768px the primary navigation docks to the bottom of the viewport.
 */
export function AppShell({
  workspace,
  user,
  nav,
  footerNav,
  context,
  breadcrumbs,
  actions,
  className,
  children,
}: AppShellProps) {
  const navigate = useNavigate()

  return (
    <EnsureThemeProvider>
      <div className={cn("app-shell", className)}>
        <aside className="app-sidebar">
          <button
            type="button"
            className="app-sidebar-workspace"
            aria-label={`Open ${workspace.name} workspace`}
            onClick={() => navigate(`/workspace/${workspace.id}`)}
          >
            <KnotreeGlyph />
            <span className="app-sidebar-workspace-name">{workspace.name}</span>
            <ChevronsUpDownIcon aria-hidden="true" />
          </button>
          {context && <div className="app-sidebar-context">{context}</div>}
          <nav className="app-sidebar-nav" aria-label="Primary">
            {nav}
            {footerNav && (
              <div className="app-sidebar-nav-footer">{footerNav}</div>
            )}
          </nav>
          <div className="app-sidebar-plan">
            <span className="app-sidebar-plan-dot" aria-hidden="true" />
            <span>
              Signed in as <strong>{user.fullName || user.email}</strong>
            </span>
          </div>
        </aside>

        <div className="app-main">
          <div className="app-panel">
            <header className="app-header">
              <Link
                to={`/workspace/${workspace.id}`}
                className="app-header-home"
                aria-hidden="true"
                tabIndex={-1}
              >
                <KnotreeGlyph />
              </Link>
              <div className="app-breadcrumbs">{breadcrumbs}</div>
              <div className="app-header-actions">
                {actions}
                <ThemeToggle />
                <AccountMenu user={user} />
              </div>
            </header>
            {children}
          </div>
        </div>
      </div>
    </EnsureThemeProvider>
  )
}

export function SidebarNavItem({
  label,
  icon,
  active = false,
  onClick,
  to,
}: {
  label: string
  icon: ReactNode
  active?: boolean
  onClick?: () => void
  to?: string
}) {
  const content = (
    <>
      <span className="app-nav-icon" aria-hidden="true">
        {icon}
      </span>
      <span className="app-nav-label">{label}</span>
    </>
  )

  if (to) {
    return (
      <Link
        to={to}
        className="app-nav-item"
        aria-current={active ? "page" : undefined}
      >
        {content}
      </Link>
    )
  }

  return (
    <button
      type="button"
      className="app-nav-item"
      aria-current={active ? "page" : undefined}
      aria-label={label}
      onClick={onClick}
    >
      {content}
    </button>
  )
}

export function BreadcrumbSeparator() {
  return (
    <span className="app-breadcrumb-separator" aria-hidden="true">
      /
    </span>
  )
}

function ThemeToggle() {
  const { theme, toggleTheme } = useTheme()
  const dark = theme === "dark"

  return (
    <button
      type="button"
      className="app-icon-button"
      aria-label={dark ? "Switch to light mode" : "Switch to dark mode"}
      aria-pressed={dark}
      title={dark ? "Light mode" : "Dark mode"}
      onClick={toggleTheme}
    >
      {dark ? <MoonIcon aria-hidden="true" /> : <SunIcon aria-hidden="true" />}
    </button>
  )
}

function AccountMenu({ user }: { user: User }) {
  const navigate = useNavigate()
  const { signOut } = useAuth()
  const [open, setOpen] = useState(false)
  const wrapRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) {
      return undefined
    }
    function handlePointerDown(event: PointerEvent) {
      if (!wrapRef.current?.contains(event.target as Node | null)) {
        setOpen(false)
      }
    }
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setOpen(false)
      }
    }
    document.addEventListener("pointerdown", handlePointerDown)
    document.addEventListener("keydown", handleKeyDown)
    return () => {
      document.removeEventListener("pointerdown", handlePointerDown)
      document.removeEventListener("keydown", handleKeyDown)
    }
  }, [open])

  async function handleSignOut() {
    try {
      await signOut()
    } finally {
      navigate("/login", { replace: true })
    }
  }

  return (
    <div ref={wrapRef} className="app-account">
      <button
        type="button"
        className="app-account-trigger"
        aria-label={`Account menu for ${user.email}`}
        aria-expanded={open}
        aria-controls="app-account-menu"
        onClick={() => setOpen((current) => !current)}
      >
        <span aria-hidden="true">{initials(user.fullName || user.email)}</span>
      </button>
      {open && (
        <div id="app-account-menu" className="app-menu app-account-menu">
          <div className="app-account-identity">
            <strong>{user.fullName || "Knotree account"}</strong>
            <span>{user.email}</span>
          </div>
          <div className="app-menu-separator" />
          <Link
            to="/settings/integrations"
            className="app-menu-item"
            onClick={() => setOpen(false)}
          >
            Integrations
          </Link>
          <button
            type="button"
            className="app-menu-item"
            onClick={() => void handleSignOut()}
          >
            <LogOutIcon aria-hidden="true" />
            Sign out
          </button>
        </div>
      )}
    </div>
  )
}

/** The Knotree mark drawn as three linked nodes, crisp at any size. */
export function KnotreeGlyph({ className }: { className?: string }) {
  return (
    <span className={cn("knotree-glyph", className)} aria-hidden="true">
      <svg viewBox="0 0 20 20" fill="none">
        <path
          d="M6 6.5 10 13.5 14 6.5"
          stroke="currentColor"
          strokeWidth="1.6"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
        <circle cx="6" cy="6" r="2.25" fill="currentColor" />
        <circle cx="14" cy="6" r="2.25" fill="currentColor" />
        <circle cx="10" cy="14" r="2.25" fill="currentColor" />
      </svg>
    </span>
  )
}
