const REGISTRY_ORIGIN = "https://registry.knotree.com"
const AUTHORIZE_PATH = /^\/cloud\/authorize\/[0-9a-f-]{36}$/

/**
 * Returns the Registry consent page to open, or throws when the API returned
 * anything other than the exact Registry authorization route.
 */
export function registryAuthorizationTarget(authorizationUrl: string) {
  const target = new URL(authorizationUrl)
  if (
    target.origin !== REGISTRY_ORIGIN ||
    !AUTHORIZE_PATH.test(target.pathname) ||
    target.search ||
    target.hash ||
    target.username ||
    target.password
  ) {
    throw new Error("Registry returned an invalid authorization URL.")
  }
  return target.href
}
