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
    <AuthShell>
      <div className="form-content">
        <div className="flex flex-col gap-2">
          <h2 className="font-heading text-3xl font-semibold tracking-[-0.03em] text-foreground">
            {isRegister ? "Create your account" : "Welcome back"}
          </h2>
          <p className="text-base text-muted-foreground">
            {isRegister
              ? "One Knotree account for Cloud, Registry and every Knotree service."
              : "Sign in with your Knotree account."}
          </p>
        </div>

        <a
          href={href}
          className={buttonVariants({ size: "lg", className: "h-11 w-full text-sm" })}
        >
          {isRegister ? "Create a Knotree account" : "Continue with Knotree"}
        </a>

        <p className="text-center text-sm text-muted-foreground">
          {isRegister ? "Already have an account?" : "Don't have an account?"}{" "}
          <Link
            to={isRegister ? "/login" : "/register"}
            className="font-medium text-link underline-offset-4 hover:underline"
          >
            {isRegister ? "Sign in" : "Create one"}
          </Link>
        </p>
      </div>
    </AuthShell>
  )
}
