import type { ReactNode } from "react"
import { BoxIcon, DatabaseIcon, LayersIcon } from "lucide-react"

import { BrandMark } from "@/components/brand-mark"

export function AuthShell({
  children,
  aside,
}: {
  children: ReactNode
  aside?: ReactNode
}) {
  return (
    <main className="auth-page">
      <div className="auth-form-column">
        <header>
          <BrandMark compact />
          {aside}
        </header>
        <section className="auth-form-panel">{children}</section>
        <footer>
          <span>© Knotree</span>
          <span>cloud.knotree.com</span>
        </footer>
      </div>
      <AuthShowcase />
    </main>
  )
}

/**
 * A quiet schematic of a project: one service talking to its data stores over
 * the private network. It previews what the product does without pretending
 * to be live data.
 */
function AuthShowcase() {
  return (
    <aside className="auth-showcase" aria-hidden="true">
      <div className="auth-diagram">
        <svg viewBox="0 0 640 352" preserveAspectRatio="none">
          <path d="M320 104 V176 H115 V248" />
          <path d="M320 104 V248" />
          <path d="M320 176 H525 V248" />
        </svg>
        <AuthNode
          left="50%"
          top="20%"
          icon={<BoxIcon />}
          name="storefront-api"
          meta="acme/api:1.8"
          status="Online"
        />
        <AuthNode
          left="18%"
          top="80%"
          icon={<DatabaseIcon />}
          name="Postgres"
          meta="postgres:5432"
          status="Online"
        />
        <AuthNode
          left="50%"
          top="80%"
          icon={<LayersIcon />}
          name="Redis"
          meta="redis:6379"
          status="Online"
        />
        <AuthNode
          left="82%"
          top="80%"
          icon={<BoxIcon />}
          name="worker"
          meta="acme/worker"
          status="Creating"
        />
      </div>
      <div className="auth-showcase-caption">
        <p>Services, databases and the network between them, in one place.</p>
        <span>production · private network</span>
      </div>
    </aside>
  )
}

function AuthNode({
  left,
  top,
  icon,
  name,
  meta,
  status,
}: {
  left: string
  top: string
  icon: ReactNode
  name: string
  meta: string
  status: "Online" | "Creating"
}) {
  return (
    <div className="auth-node" style={{ left, top }}>
      <div className="auth-node-head">
        {icon}
        <span>{name}</span>
      </div>
      <div className="auth-node-meta">
        <span>{meta}</span>
        <span className={`status-text status-${status.toLowerCase()}`}>
          {status}
        </span>
      </div>
    </div>
  )
}
