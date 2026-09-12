# Knotree Cloud web app

Vite + React Router + shadcn/Base UI + Tailwind CSS frontend for the Knotree
Cloud authentication and workspace onboarding flow.

```powershell
pnpm dev
pnpm typecheck
pnpm test
pnpm build
```

Set `VITE_API_BASE_URL` in `.env.local` for a different API origin. Production
builds should use `https://cloudapi.knotree.com/api/v1`; the generated static
assets are deployed with `wrangler.jsonc` to `cloud.knotree.com`.
