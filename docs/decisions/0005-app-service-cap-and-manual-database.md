---
created_at: "2026-09-13T13:37:35.487449800+00:00"
doc: docs/decisions/0005-app-service-cap-and-manual-database.md
id: DEC-0005
notes: "Each project may own at most six Docker App services, excluding its PostgreSQL resource. New services have no database_resource_id until the user assigns or removes a project-ready PostgreSQL resource through the service Settings menu; database creation never triggers automatic app redeployment."
status: accepted
title: Six app services with explicit database attachment
type: decision
updated_at: "2026-09-13T13:37:35.487455200+00:00"
verify: cargo test --manifest-path apps/api/Cargo.toml; pnpm --dir apps/web test -- --run
---

# Six app services with explicit database attachment
