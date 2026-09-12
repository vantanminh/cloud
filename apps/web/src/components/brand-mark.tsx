import { cn } from "cn"

type BrandMarkProps = {
  compact?: boolean
  className?: string
}

export function BrandMark({ compact = false, className }: BrandMarkProps) {
  return (
    <div className={cn("flex items-center gap-3", className)}>
      <img
        src="/brand/knotree-mark.png"
        alt=""
        className={cn("size-11 object-contain", compact && "size-8")}
      />
      <span
        className={cn(
          "font-heading text-xl font-semibold tracking-tight",
          compact && "text-base"
        )}
      >
        Knotree Cloud
      </span>
    </div>
  )
}
