---
created_at: "2026-09-13T06:30:09.627693+00:00"
doc: docs/decisions/0004-private-project-networking.md
id: "0004"
links:
  - US-010
  - docs/database-system-design.md
  - docs/frontend-system-design.md
notes: "Docker App and PostgreSQL containers share one deterministic bridge network per project, use postgres/app DNS aliases, and app provisioning receives DATABASE_URL plus PG* variables from the ready project's encrypted database credential. Host-published ports remain for local/API access; Kubernetes App service provisioning stays disabled until an equivalent namespace-scoped implementation exists."
status: accepted
title: Project-scoped private networking for Docker resources
type: decision
updated_at: "2026-09-13T06:30:09.627694300+00:00"
verify: "Runtime smoke tests prove both creation orders, private DNS resolution, credentialed SELECT 1 over postgres:5432, and no secret values in App service responses."
---

# Project-scoped private networking for Docker resources
