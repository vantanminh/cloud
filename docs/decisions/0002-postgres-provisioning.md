---
created_at: "2026-09-12T13:37:50.607901300+00:00"
doc: docs/decisions/0002-postgres-provisioning.md
id: DEC-0002
links:
  - US-003
  - docs/frontend-system-design.md
notes: "The first resource provider uses the PostgreSQL cluster in DATABASE_URL. It creates one generated database and one non-privileged login role per project using CREATE ROLE and CREATE DATABASE outside a transaction, serializes project provisioning with pg_advisory_xact_lock, stores only AES-256-GCM encrypted password material, and requires CREATEDB/CREATEROLE for the API role. DATABASE_RESOURCE_HOST may override the client-reachable host. This is an explicit cluster provider; managed providers without role/database creation privileges need a future provider adapter."
status: accepted
title: Provision isolated PostgreSQL databases per project
type: decision
updated_at: "2026-09-12T13:37:50.607906300+00:00"
verify: "Runtime API smoke test creates a database and login role on compose Postgres, authenticates with returned credentials, and verifies idempotent retry"
---

# Provision isolated PostgreSQL databases per project
