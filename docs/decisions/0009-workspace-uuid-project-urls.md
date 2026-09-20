---
created_at: "2026-09-20T04:00:42.294689757+00:00"
doc: docs/decisions/0009-workspace-uuid-project-urls.md
id: DEC-0009
links:
  - IN-039
  - US-041
notes: Remove user-defined workspace slugs and use the immutable workspace UUID in every workspace route and API path. Preserve readable project URLs by deriving a project slug from its name; allow a custom slug and rely on the per-workspace unique database constraint for collisions.
status: accepted
title: Use workspace UUIDs and name-derived project URLs
type: decision
updated_at: "2026-09-20T04:00:42.294690739+00:00"
verify: "Workspace responses omit slug; workspace routes and API paths use the workspace UUID; project slug is generated from the name by default, optionally customized, and unique within a workspace."
---

# Use workspace UUIDs and name-derived project URLs
