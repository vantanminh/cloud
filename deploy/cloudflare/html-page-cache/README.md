# HTML page cache Worker

This Worker is deployed separately from the API and web images in k3s. The
`wrangler.jsonc` route sends `*.knotree.org/*` requests through it; the Worker
caches eligible `GET` responses for `page-*.knotree.org` and passes other hosts
and methods through.

Install Wrangler from the lockfile and deploy from this directory:

```sh
npm ci
npm run deploy
```

Wrangler must be authenticated to the Cloudflare account that owns `knotree.org`.
Keep deployment credentials outside the repository. The root GitHub Actions
workflow currently deploys the API and web images but does not deploy this
Worker.
