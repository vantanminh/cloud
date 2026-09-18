---
created_at: "2026-09-18T11:48:21.021474581+00:00"
doc: docs/decisions/0008-virtual-app-cpu.md
id: DEC-0008
links:
  - US-036
  - IN-035
notes: "Keep the product-facing App service allocation at 1 virtual vCPU, 1Gi RAM, and 10Gi storage, but do not reserve one physical CPU per service on a small VPS. Kubernetes App pods use minimal scheduler requests (1m CPU, 32Mi memory, 64Mi ephemeral storage) and a 250m host CPU limit. The displayed 1 vCPU is a virtual plan unit; Postgres and Redis limits remain unchanged."
status: accepted
title: Virtual CPU allocation for Kubernetes App services
type: decision
updated_at: "2026-09-18T11:48:21.021475669+00:00"
verify: API unit tests assert the App Deployment manifest uses the virtual CPU requests and 250m host limit; rendered Settings text labels App CPU as virtual; production rollout should redeploy App workloads to pick up the manifest.
---

# Virtual CPU allocation for Kubernetes App services
