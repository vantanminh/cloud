import assert from "node:assert/strict"
import { createServer } from "node:http"
import test from "node:test"

import { KnotreeImagesError, createKnotreeImages } from "./client.js"

function listen(handler) {
  const server = createServer(handler)
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      const address = server.address()
      resolve({
        server,
        baseUrl: `http://127.0.0.1:${address.port}/api/v1`,
      })
    })
  })
}

function close(server) {
  return new Promise((resolve, reject) => {
    server.close((error) => (error ? reject(error) : resolve()))
  })
}

async function readBody(request) {
  const chunks = []
  for await (const chunk of request) {
    chunks.push(chunk)
  }
  return Buffer.concat(chunks)
}

test("uploads, lists, signs, and deletes with client credentials", async () => {
  const seen = []
  const { server, baseUrl } = await listen(async (request, response) => {
    const url = new URL(request.url, "http://127.0.0.1")
    const path = url.pathname.replace(/^\/api\/v1/, "")
    seen.push({
      method: request.method,
      path: url.pathname,
      query: url.searchParams.toString(),
      clientId: request.headers["x-knotree-client-id"],
      clientSecret: request.headers["x-knotree-client-secret"],
      folder: request.headers["x-knotree-folder"],
      fileName: request.headers["x-knotree-file-name"],
      contentType: request.headers["content-type"],
      body: await readBody(request),
    })
    if (request.method === "DELETE" && path.endsWith("/objects/img-1")) {
      response.writeHead(204)
      response.end()
      return
    }
    const payloads = {
      "GET /images/store": { id: "store", compressionMode: "per_url" },
      "POST /images/objects": { id: "img-1", folder: "posts", fileName: "cover.png" },
      "GET /images/objects": { objects: [{ id: "img-1" }], folders: ["posts"] },
      "DELETE /images/folders": { deleted: 2 },
      "POST /images/objects/img-1/sign": {
        url: "https://img.knotree.org/images/v1/store/img-1?mode=per_url&w=800&q=70&sig=abc",
        visibility: "public",
        expiresAt: null,
        cacheSeconds: 31536000,
        contentType: "image/webp",
      },
    }
    const payload = payloads[`${request.method} ${path}`]
    response.writeHead(payload ? 201 : 404, { "Content-Type": "application/json" })
    response.end(JSON.stringify(payload ?? { error: { code: "MISSING", message: "missing" } }))
  })

  try {
    const images = createKnotreeImages({
      clientId: "kimg_browser",
      clientSecret: "ksec_secret",
      baseUrl,
    })
    assert.equal(images.baseUrl, baseUrl.replace(/\/api\/v1$/, ""))
    assert.deepEqual(await images.getStore(), {
      id: "store",
      compressionMode: "per_url",
    })
    assert.equal(
      (
        await images.upload({
          body: Buffer.from("png"),
          contentType: "image/png",
          folder: "posts",
          fileName: "cover.png",
        })
      ).id,
      "img-1"
    )
    assert.deepEqual(await images.list({ folder: "posts", recursive: true }), {
      objects: [{ id: "img-1" }],
      folders: ["posts"],
    })
    const signed = await images.signUrl("img-1", {
      visibility: "public",
      width: 800,
      quality: 70,
    })
    assert.equal(signed.cacheSeconds, 31536000)
    assert.match(signed.url, /^https:\/\/img\.knotree\.org\//)
    assert.equal(await images.delete("img-1"), null)
    assert.deepEqual(await images.deleteFolder("posts"), { deleted: 2 })
  } finally {
    await close(server)
  }

  assert.equal(seen.length, 6)
  for (const call of seen) {
    assert.equal(call.clientId, "kimg_browser")
    assert.equal(call.clientSecret, "ksec_secret")
  }
  const upload = seen.find((call) => call.method === "POST" && call.path === "/api/v1/images/objects")
  assert.equal(upload.folder, "posts")
  assert.equal(upload.fileName, "cover.png")
  assert.equal(upload.contentType, "image/png")
  assert.equal(upload.body.toString(), "png")
  const list = seen.find((call) => call.method === "GET" && call.path === "/api/v1/images/objects")
  assert.equal(list.query, "folder=posts&recursive=true")
  const sign = seen.find((call) => call.path.endsWith("/sign"))
  assert.deepEqual(JSON.parse(sign.body.toString()), {
    visibility: "public",
    width: 800,
    quality: 70,
  })
})

test("surfaces revoked keys and keeps the caller from storing image bytes", async () => {
  const { server, baseUrl } = await listen((_request, response) => {
    response.writeHead(401, { "Content-Type": "application/json" })
    response.end(
      JSON.stringify({
        error: {
          code: "IMAGE_KEY_REVOKED",
          message: "This image client key has been revoked.",
        },
      })
    )
  })
  try {
    const images = createKnotreeImages({
      clientId: "kimg_browser",
      clientSecret: "ksec_secret",
      baseUrl,
    })
    await assert.rejects(
      () => images.signUrl("img-1", { visibility: "private", expiresInSeconds: 60 }),
      (error) => {
        assert.ok(error instanceof KnotreeImagesError)
        assert.equal(error.status, 401)
        assert.equal(error.code, "IMAGE_KEY_REVOKED")
        return true
      }
    )
  } finally {
    await close(server)
  }
})

test("requires a client id, client secret, and file name", () => {
  assert.throws(
    () => createKnotreeImages({ clientId: "", clientSecret: "ksec", baseUrl: "http://localhost" }),
    (error) => error instanceof KnotreeImagesError && error.code === "IMAGE_CONFIG"
  )
  const images = createKnotreeImages({
    clientId: "kimg",
    clientSecret: "ksec",
    baseUrl: "http://localhost:8080",
  })
  assert.throws(
    () => images.upload({ body: new Uint8Array(), fileName: "" }),
    (error) => error.code === "IMAGE_CONFIG"
  )
})
