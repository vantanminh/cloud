import { Spinner } from "@/components/ui/spinner"

export function LoadingScreen() {
  return (
    <main className="grid min-h-svh place-items-center bg-canvas">
      <Spinner
        aria-label="Loading Knotree Cloud"
        className="size-6 text-primary"
      />
    </main>
  )
}
