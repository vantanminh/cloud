---
created_at: "2026-09-15T16:36:39.892407400+00:00"
doc: docs/decisions/0006-public-app-service-domains.md
id: DEC-0006
notes: "Configure APP_SERVICE_PUBLIC_DOMAIN with the DNS root (for example knotree.org). Assign each App service a stable random public subdomain, expose the wildcard at the API ingress, and resolve the Host subdomain to the service record before proxying the original request path."
status: accepted
title: Wildcard host routing for public App services
type: decision
updated_at: "2026-09-15T16:36:39.892414800+00:00"
verify: null
---

# Wildcard host routing for public App services
