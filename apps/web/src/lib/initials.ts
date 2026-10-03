/** Up to two uppercase letters for an avatar or project tile. */
export function initials(value: string) {
  const words = value
    .trim()
    .split(/[\s@._-]+/)
    .filter(Boolean)
  const letters = (words[0]?.[0] ?? "K") + (words[1]?.[0] ?? "")
  return letters.toUpperCase()
}
