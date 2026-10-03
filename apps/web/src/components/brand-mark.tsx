import { cn } from "cn"

import { KnotreeGlyph } from "@/components/app-shell"

type BrandMarkProps = {
  compact?: boolean
  className?: string
}

export function BrandMark({ compact = false, className }: BrandMarkProps) {
  return (
    <div className={cn("flex items-center gap-2.5", className)}>
      <KnotreeGlyph className={cn(!compact && "size-7")} />
      <span
        className={cn(
          "text-[0.9375rem] font-semibold tracking-[-0.015em]",
          !compact && "text-base"
        )}
      >
        Knotree Cloud
      </span>
    </div>
  )
}
