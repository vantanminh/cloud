# HTML page cache Worker

This Worker is deployed separately from the API and web images in k3s. The
`wrangler.jsonc` route sends `*.knotree.org/*` requests through it; the Worker
caches eligible `GET` responses for `page-*.knotree.org` and for
`img.knotree.org`. Public image URLs keep the origin cache lifetime of one
year. Private image URLs, identified by `exp` or `kid`, are stored for 60
seconds. Image cache entries use one shared CORS header so a response cached
for one website can be reused by another. Other hosts and methods pass through.

Install Wrangler from the lockfile and deploy from this directory:

```sh
npm ci
npm run deploy
```

Wrangler must be authenticated to the Cloudflare account that owns `knotree.org`.
Keep deployment credentials outside the repository. The root GitHub Actions
workflow currently deploys the API and web images but does not deploy this
Worker.
