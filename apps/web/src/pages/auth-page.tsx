import { ArrowRightIcon, CheckIcon } from "lucide-react"
import { Link } from "react-router-dom"

import { AuthShell } from "@/components/auth-shell"
import { buttonVariants } from "@/components/ui/button"
import { apiUrl } from "@/lib/api"

type AuthMode = "login" | "register"

/**
 * Cloud has no passwords of its own: one Knotree account signs in to every
 * Knotree service, so both doors lead to Knotree Accounts.
 */
export function AuthPage({ mode }: { mode: AuthMode }) {
  const isRegister = mode === "register"
  const href = apiUrl(
    isRegister ? "/auth/sso/start?intent=signup" : "/auth/sso/start"
  )

  return (
    <AuthShell
      aside={
        <p className="text-[0.8125rem] text-muted-foreground">
          <span className="auth-switch-prompt">
            {isRegister
              ? "Already have an account?"
              : "Don't have an account?"}{" "}
          </span>
          <Link
            to={isRegister ? "/login" : "/register"}
            className="font-medium text-foreground underline-offset-4 hover:underline"
          >
            {isRegister ? "Sign in" : "Create one"}
          </Link>
        </p>
      }
    >
      <div className="form-content">
        <div className="auth-heading">
          <h1>{isRegister ? "Create your account" : "Welcome back"}</h1>
          <p>
            {isRegister
              ? "One Knotree account for Cloud, Registry and every Knotree service."
              : "Sign in with your Knotree account to open your workspace."}
          </p>
        </div>

        <a
          href={href}
          className={buttonVariants({
            size: "lg",
            className: "h-10 w-full text-sm",
          })}
        >
          {isRegister ? "Create a Knotree account" : "Continue with Knotree"}
          <ArrowRightIcon data-icon="inline-end" aria-hidden="true" />
        </a>

        <ul className="auth-includes">
          <li>
            <CheckIcon aria-hidden="true" />
            Deploy containers and static pages
          </li>
          <li>
            <CheckIcon aria-hidden="true" />
            Managed PostgreSQL and Redis on a private network
          </li>
          <li>
            <CheckIcon aria-hidden="true" />
            Live metrics and deployment logs
          </li>
        </ul>
      </div>
    </AuthShell>
  )
}
