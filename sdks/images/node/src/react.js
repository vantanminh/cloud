import { createElement, useEffect, useRef, useState } from "react"

export function useKnotreeImageUrl(client, imageId, options = {}) {
  const visibility = options.visibility ?? "public"
  const expiresInSeconds = options.expiresInSeconds
  const width = options.width
  const height = options.height
  const quality = options.quality
  const enabled = options.enabled ?? true
  const clientRef = useRef(client)
  clientRef.current = client
  const [state, setState] = useState({
    url: null,
    signed: null,
    error: null,
    loading: Boolean(enabled && imageId),
  })

  useEffect(() => {
    if (!enabled || !imageId) {
      setState({ url: null, signed: null, error: null, loading: false })
      return undefined
    }
    let active = true
    setState((current) => ({ ...current, loading: true, error: null }))
    clientRef.current
      .signUrl(imageId, {
        visibility,
        expiresInSeconds,
        width,
        height,
        quality,
      })
      .then((signed) => {
        if (active) {
          setState({
            url: signed.url,
            signed,
            error: null,
            loading: false,
          })
        }
      })
      .catch((error) => {
        if (active) {
          setState({ url: null, signed: null, error, loading: false })
        }
      })
    return () => {
      active = false
    }
  }, [
    client.baseUrl,
    client.clientId,
    client.clientSecret,
    enabled,
    expiresInSeconds,
    height,
    imageId,
    quality,
    visibility,
    width,
  ])

  return state
}

export function KnotreeImage({
  client,
  imageId,
  alt = "",
  visibility,
  expiresInSeconds,
  width,
  height,
  quality,
  enabled,
  ...rest
}) {
  const { url, error, loading } = useKnotreeImageUrl(client, imageId, {
    visibility,
    expiresInSeconds,
    width,
    height,
    quality,
    enabled,
  })
  if (!url) {
    return createElement("span", {
      role: "img",
      "aria-label": alt,
      "data-knotree-image-loading": loading ? "true" : "false",
      "data-knotree-image-error": error?.code || undefined,
    })
  }
  return createElement("img", { ...rest, alt, src: url })
}
