---
created_at: "2026-09-18T11:48:21.021474581+00:00"
doc: docs/decisions/0008-virtual-app-cpu.md
id: DEC-0008
links:
  - US-036
  - IN-035
notes: "Keep the product-facing App service allocation at 1 virtual vCPU, 1Gi RAM, and 10Gi storage, but do not reserve one physical CPU per service on a small VPS. Kubernetes App pods use explicit zero scheduler requests (rather than omitting requests, which can inherit limits) and a 250m host CPU limit. The API runs a reconciler that strategically patches existing App Deployments after rollout. The displayed 1 vCPU is a virtual plan unit; Postgres and Redis limits remain unchanged."
status: accepted
title: Virtual CPU allocation for Kubernetes App services
type: decision
updated_at: "2026-09-18T12:00:59.540348351+00:00"
verify: API unit tests assert the App Deployment manifest and reconciler patch use explicit zero requests and a 250m host limit; kubectl server-side dry-run accepts zero requests and strategic patch preserves image/port/env; rendered Settings text labels App CPU as virtual; production API rollout starts the reconciler.
---

# Virtual CPU allocation for Kubernetes App services
