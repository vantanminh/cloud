import { useState } from "react"
import { EyeIcon, EyeOffIcon } from "lucide-react"

import { Field, FieldError, FieldLabel } from "@/components/ui/field"
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group"

type PasswordFieldProps = {
  id: string
  label: string
  value: string
  placeholder: string
  autoComplete: string
  error?: string
  onChange: (value: string) => void
}

export function PasswordField({
  id,
  label,
  value,
  placeholder,
  autoComplete,
  error,
  onChange,
}: PasswordFieldProps) {
  const [visible, setVisible] = useState(false)

  return (
    <Field data-invalid={Boolean(error)}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      <InputGroup>
        <InputGroupInput
          id={id}
          type={visible ? "text" : "password"}
          value={value}
          placeholder={placeholder}
          autoComplete={autoComplete}
          aria-invalid={Boolean(error)}
          onChange={(event) => onChange(event.target.value)}
        />
        <InputGroupAddon align="inline-end">
          <InputGroupButton
            aria-label={visible ? "Hide password" : "Show password"}
            aria-pressed={visible}
            onClick={() => setVisible((current) => !current)}
            size="icon-sm"
          >
            {visible ? (
              <EyeOffIcon data-icon="inline-start" />
            ) : (
              <EyeIcon data-icon="inline-start" />
            )}
          </InputGroupButton>
        </InputGroupAddon>
      </InputGroup>
      {error && <FieldError>{error}</FieldError>}
    </Field>
  )
}
