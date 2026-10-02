# Image stores

An image store keeps website images in Knotree Cloud. A project can upload,
list, delete, and serve images in folders. Firebase, Supabase, or any other
backend stores the signed URL. Knotree Cloud stores the bytes.

## Credentials

Create a client key in the project dashboard. The client id starts with `kimg_`
and the client secret starts with `ksec_`. The secret is shown once and stored
only as a hash.

Send either both headers:

```text
X-Knotree-Client-Id: kimg_...
X-Knotree-Client-Secret: ksec_...
```

or `Authorization: Bearer kimg_...:ksec_...`.

A browser key can upload, list, and sign public URLs. Put that key in a
Next.js or React client when the site uses a backend that should not store
image bytes. A full key can also delete images, delete folders, and sign
private URLs. Keep full keys on a server.

Revoking a key disables upload, delete, list, and every new signature. Private
URLs signed by that key stop working. Public URLs that were already signed keep
working, because those URLs are signed with a platform key rather than the
client secret.

## Serving

Public URLs are stable. They do not expire. The response asks caches to keep
the image for one year (`Cache-Control: public, max-age=31536000, immutable`).
The `img.knotree.org` Cloudflare Worker stores that response for the same
period.

Private URLs include an expiry of up to 7 days and the client id that signed
them. They are cached for 60 seconds. After the key is revoked or the expiry
passes, the URL no longer serves the image.

Signed URLs look like:

```text
https://img.knotree.org/images/v1/{storeId}/{imageId}?mode=none&sig=...
https://img.knotree.org/images/v1/{storeId}/{imageId}?mode=per_url&w=800&q=70&sig=...
https://img.knotree.org/images/v1/{storeId}/{imageId}?mode=fixed&w=1600&q=80&exp=...&kid=kimg_...&sig=...
```

## Compression

Each store has one compression mode:

- `none` keeps the original PNG, JPEG, GIF, or WebP bytes.
- `fixed` converts every served image to WebP using the store's width, height,
  and quality. A URL cannot override those values.
- `per_url` converts to WebP and lets each signature choose width, height, and
  quality, capped by the store.

Changing the store setting later does not change a URL that was already signed.
The transform is part of the signature.

## Developer API

The API origin is `https://cloudapi.knotree.com` in production and
`http://localhost:8080` in development. Paths are under `/api/v1/images`.

- `GET /images/store`
- `POST /images/objects` with the raw image body, `Content-Type`,
  `X-Knotree-Folder`, and `X-Knotree-File-Name`
- `GET /images/objects?folder=&recursive=true`
- `DELETE /images/objects/{imageId}`
- `DELETE /images/folders?folder=`
- `POST /images/objects/{imageId}/sign` with
  `{ "visibility": "public" | "private", "expiresInSeconds": 3600, "width": 800, "height": 600, "quality": 70 }`

Clients for Node.js, Rust, and Go live in `sdks/images`.

## Dashboard

Session routes under
`/api/v1/workspaces/{workspaceId}/projects/{projectSlug}/image-stores` create
stores and keys, upload from the browser session, and sign URLs. Signing a
private URL from the dashboard requires an active full-access key.
