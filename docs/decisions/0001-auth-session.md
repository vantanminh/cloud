---
created_at: "2026-09-12T10:26:40.346392300+00:00"
doc: docs/decisions/0001-auth-session.md
id: DEC-0001
notes: "Use Axum/SQLx/Postgres with opaque random session tokens stored as hashes, HttpOnly host-only cookies, exact CORS for cloud.knotree.com, Origin plus double-submit CSRF protection, and 30-day sliding expiry. Dev disables email verification; production policy is a guarded future hook."
status: accepted
title: Opaque cookie sessions and cross-origin SPA contract
type: decision
updated_at: "2026-09-12T10:26:40.346394500+00:00"
verify: null
---

# Opaque cookie sessions and cross-origin SPA contract
