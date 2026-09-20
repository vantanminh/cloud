import { useState, type FormEvent } from "react"
import { Link, useNavigate } from "react-router-dom"

import { useAuth } from "@/auth/auth-context"
import { AuthShell } from "@/components/auth-shell"
import { PasswordField } from "@/components/password-field"
import { Alert, AlertDescription } from "@/components/ui/alert"
import { Button } from "@/components/ui/button"
import {
  Field,
  FieldError,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { ApiError } from "@/lib/api"
import type { AuthResponse } from "@/lib/types"

type AuthMode = "login" | "register"
type FormErrors = Record<string, string>

export function AuthPage({ mode }: { mode: AuthMode }) {
  const navigate = useNavigate()
  const { signIn, signUp } = useAuth()
  const [fullName, setFullName] = useState("")
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [errors, setErrors] = useState<FormErrors>({})
  const [submitError, setSubmitError] = useState<string | null>(null)
  const [isSubmitting, setIsSubmitting] = useState(false)

  const isRegister = mode === "register"

  async function handleSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const nextErrors = validateForm({ fullName, email, password, isRegister })
    setErrors(nextErrors)
    setSubmitError(null)
    if (Object.keys(nextErrors).length > 0) {
      return
    }

    setIsSubmitting(true)
    try {
      const session = isRegister
        ? await signUp({ fullName: fullName.trim(), email: email.trim(), password })
        : await signIn({ email: email.trim(), password })
      navigate(destinationFor(session), { replace: true })
    } catch (error) {
      if (error instanceof ApiError) {
        setErrors(error.fields)
        setSubmitError(error.fields.email ? null : error.message)
      } else {
        setSubmitError("The API is currently unavailable. Please try again.")
      }
    } finally {
      setIsSubmitting(false)
    }
  }

  return (
    <AuthShell>
      <div className="form-content">
        <div className="flex flex-col gap-2">
          <h2 className="font-heading text-3xl font-semibold tracking-[-0.03em] text-foreground">
            {isRegister ? "Create your account" : "Welcome back"}
          </h2>
          <p className="text-base text-muted-foreground">
            {isRegister
              ? "Start with a workspace for your ideas."
              : "Sign in to your workspace"}
          </p>
        </div>

        {submitError && (
          <Alert variant="destructive">
            <AlertDescription>{submitError}</AlertDescription>
          </Alert>
        )}

        <form className="flex flex-col gap-6" onSubmit={handleSubmit} noValidate>
          <FieldGroup>
            {isRegister && (
              <Field data-invalid={Boolean(errors.fullName)}>
                <FieldLabel htmlFor="fullName">Full name</FieldLabel>
                <Input
                  id="fullName"
                  name="fullName"
                  value={fullName}
                  placeholder="Jane Doe"
                  autoComplete="name"
                  aria-invalid={Boolean(errors.fullName)}
                  onChange={(event) => setFullName(event.target.value)}
                />
                {errors.fullName && <FieldError>{errors.fullName}</FieldError>}
              </Field>
            )}
            <Field data-invalid={Boolean(errors.email)}>
              <FieldLabel htmlFor="email">
                {isRegister ? "Work email" : "Email"}
              </FieldLabel>
              <Input
                id="email"
                name="email"
                type="email"
                value={email}
                placeholder="you@company.com"
                autoComplete="email"
                aria-invalid={Boolean(errors.email)}
                onChange={(event) => setEmail(event.target.value)}
              />
              {errors.email && <FieldError>{errors.email}</FieldError>}
            </Field>
            <PasswordField
              id="password"
              label="Password"
              value={password}
              placeholder={isRegister ? "Create a password" : "Enter your password"}
              autoComplete={isRegister ? "new-password" : "current-password"}
              error={errors.password}
              onChange={setPassword}
            />
          </FieldGroup>

          <Button
            type="submit"
            size="lg"
            disabled={isSubmitting}
            className="h-11 w-full text-sm"
          >
            {isSubmitting && <Spinner data-icon="inline-start" />}
            {isRegister ? "Create account" : "Sign in"}
          </Button>
        </form>

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

function validateForm({
  fullName,
  email,
  password,
  isRegister,
}: {
  fullName: string
  email: string
  password: string
  isRegister: boolean
}): FormErrors {
  const nextErrors: FormErrors = {}
  if (isRegister && (fullName.trim().length === 0 || fullName.trim().length > 80)) {
    nextErrors.fullName = "Enter a name between 1 and 80 characters."
  }
  if (!/^\S+@\S+\.\S+$/.test(email.trim())) {
    nextErrors.email = "Enter a valid email address."
  }
  const passwordLength = Array.from(password).length
  if (isRegister && (passwordLength < 8 || passwordLength > 128)) {
    nextErrors.password = "Use 8 to 128 characters."
  } else if (!isRegister && passwordLength === 0) {
    nextErrors.password = "Password is required."
  }
  return nextErrors
}

function destinationFor(session: AuthResponse) {
  return session.workspace
    ? `/workspace/${session.workspace.id}`
    : "/new/workspace"
}
