---
created_at: "2026-09-16T15:13:13.598849841+00:00"
doc: docs/decisions/0007-paas-isolation-and-github-deploy.md
id: DEC-0007
links:
  - IN-026
  - US-023
  - US-024
  - US-025
  - US-026
  - US-027
notes: "Tenant App/Postgres/Redis share 1 vCPU / 1Gi / 10Gi caps applied at provision time. Public *.knotree.org is assigned only when enabled. A dedicated Kong instance in the release enforces per-App rate limits. MCP uses OAuth authorization code + refresh tokens. CI on GitHub builds and pushes images; k3s only pulls."
status: accepted
title: "Kubernetes tenant workloads, Kong rate limits, opt-in public domains, Redis, MCP OAuth, GitHub-only deploy"
type: decision
updated_at: "2026-09-16T15:13:13.598851247+00:00"
verify: null
---

# Kubernetes tenant workloads, Kong rate limits, opt-in public domains, Redis, MCP OAuth, GitHub-only deploy
