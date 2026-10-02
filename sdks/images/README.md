# Knotree Cloud image clients

These clients call the Knotree Cloud image API with a client id and client
secret from an image store. They upload, list, delete, and sign URLs. They do
not store image bytes in Firebase, Supabase, or another database. Save the
`url` returned by `signUrl`.

`baseUrl` is the API origin, such as `https://cloudapi.knotree.com` or
`http://localhost:8080`. A value that already ends in `/api/v1` is accepted.
Signed image URLs use `https://img.knotree.org` and can be loaded directly by
a website.

Use a browser key in React and Next.js. It can upload, list, and sign public
URLs. Use a full key on a server when the application deletes images or signs
private URLs. After a key is revoked, new calls fail. Public URLs signed
before revocation continue to load.

## Node.js, Next.js, and React

```js
import { createKnotreeImages } from "@knotree/images"
import { KnotreeImage } from "@knotree/images/react"

const images = createKnotreeImages({
  clientId: process.env.NEXT_PUBLIC_KNOTREE_IMAGE_CLIENT_ID,
  clientSecret: process.env.NEXT_PUBLIC_KNOTREE_IMAGE_CLIENT_SECRET,
  baseUrl: "https://cloudapi.knotree.com",
})

const uploaded = await images.upload({
  body: file,
  contentType: file.type,
  folder: "posts",
  fileName: file.name,
})
const signed = await images.signUrl(uploaded.id, {
  visibility: "public",
  width: 800,
  quality: 70,
})
await setDoc(postRef, { coverUrl: signed.url })
```

In a client component:

```jsx
"use client"

<KnotreeImage client={images} imageId={post.imageId} alt="" width={800} />
```

`useKnotreeImageUrl(client, imageId, options)` returns `{ url, signed, error, loading }`.
It signs again when the client id, client secret, API origin, image id, or
transform options change.

Other methods: `getStore()`, `list({ folder, recursive })`, `delete(imageId)`,
and `deleteFolder(folder)`.

## Rust

```rust
let images = knotree_images::Client::new(
    "kimg_...",
    "ksec_...",
    "https://cloudapi.knotree.com",
)?;
let object = images.upload(knotree_images::Upload {
    bytes: &bytes,
    content_type: "image/png",
    folder: "posts",
    file_name: "cover.png",
}).await?;
let signed = images.sign_url(&object.id, knotree_images::SignOptions::public_url()).await?;
```

The crate is `sdks/images/rust`.

## Go

```go
images, err := knotreeimages.New("kimg_...", "ksec_...", "https://cloudapi.knotree.com")
object, err := images.Upload(ctx, knotreeimages.Upload{
    Bytes: bytes, ContentType: "image/png", Folder: "posts", FileName: "cover.png",
})
signed, err := images.SignURL(ctx, object.ID, knotreeimages.SignOptions{Visibility: "public"})
```

The module is `github.com/vantanminh/cloud/sdks/images/go`.
