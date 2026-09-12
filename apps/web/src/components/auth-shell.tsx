import type { ReactNode } from "react"

import { BrandMark } from "@/components/brand-mark"

export function AuthShell({ children }: { children: ReactNode }) {
  return (
    <main className="auth-page">
      <div className="auth-frame">
        <aside className="auth-brand-panel">
          <BrandMark />
          <div className="relative z-10 flex max-w-xs flex-col gap-4 lg:mt-auto">
            <h1 className="font-heading text-4xl leading-[1.05] font-semibold tracking-[-0.04em] text-foreground sm:text-5xl">
              A calmer way to build together
            </h1>
            <p className="max-w-[18rem] text-base leading-7 text-muted-foreground">
              Knotree Cloud gives teams a flexible workspace to turn ideas into structure.
            </p>
          </div>
          <img src="/brand/knotree-lines.png" alt="" className="auth-motif" />
          <p className="relative z-10 text-xs tracking-wide text-muted-foreground">
            Ideas. Structure. Progress together.
          </p>
        </aside>
        <section className="auth-form-panel">{children}</section>
      </div>
    </main>
  )
}
