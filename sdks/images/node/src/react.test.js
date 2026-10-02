import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"

test("the React entry signs through the same client and renders an image", () => {
  const source = readFileSync(new URL("./react.js", import.meta.url), "utf8")
  assert.match(source, /export function useKnotreeImageUrl/)
  assert.match(source, /export function KnotreeImage/)
  assert.match(source, /signUrl/)
  assert.match(source, /createElement\("img"/)
  assert.match(source, /client\.clientId/)
  assert.match(source, /client\.clientSecret/)
  assert.match(source, /client\.baseUrl/)
})
