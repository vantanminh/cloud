---
created_at: "2026-09-13T03:14:06.509200500+00:00"
doc: docs/decisions/0003-dedicated-postgres-cluster.md
id: DEC-0003
links:
  - US-005
  - docs/frontend-system-design.md
  - deploy/helm/knotree-api
notes: "Use a provider abstraction. Development uses a host Docker daemon with one postgres:16 container and named volume per project. Kubernetes production uses one StatefulSet, PVC, Secret and Service per project. The control-plane PostgreSQL stores only encrypted credentials and cluster metadata; it is never used as a project database. A single replica is a dedicated cluster boundary, not HA; replication/failover is a future provider capability."
status: accepted
title: Dedicated PostgreSQL cluster per project
type: decision
updated_at: "2026-09-13T03:14:06.509202600+00:00"
verify: "Local Docker integration proves separate container/process/volume/port and cross-project access is rejected; Helm manifests render a per-project StatefulSet, PVC, Service, Secret and API RBAC provider contract."
---

# Dedicated PostgreSQL cluster per project
